// SPDX-License-Identifier: GPL-3.0-or-later
// Records one process's own audio (and its children's) to a 32-bit float WAV: Windows' process
// loopback (Windows 10 2004 and later), so nothing else playing on the PC is in it. For the tests:
// crackling, clipping and dropouts in Skyrim's mix.
//
//   audio_capture.exe <pid> <out.wav> <max seconds>
//
// It stops at the time limit or when the process exits, and writes <out.wav>.start with the local
// time of the file's first sample (HH:MM:SS.mmm), to line the file up with the plugin's log.
// Silences the process doesn't send are filled in, so the file's clock is the wall clock's.
#define NOMINMAX
#define _CRT_SECURE_NO_WARNINGS
#include <windows.h>

#include <audioclient.h>
#include <audioclientactivationparams.h>
#include <mmdeviceapi.h>

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace
{
	// ActivateAudioInterfaceAsync calls this back (on another thread) once the client is made.
	class Activated : public IActivateAudioInterfaceCompletionHandler, public IAgileObject
	{
	public:
		HANDLE done = CreateEventW(nullptr, TRUE, FALSE, nullptr);

		STDMETHODIMP QueryInterface(REFIID a_iid, void** a_out) override
		{
			if (a_iid == __uuidof(IUnknown) || a_iid == __uuidof(IActivateAudioInterfaceCompletionHandler)) {
				*a_out = static_cast<IActivateAudioInterfaceCompletionHandler*>(this);
			} else if (a_iid == __uuidof(IAgileObject)) {
				*a_out = static_cast<IAgileObject*>(this);
			} else {
				*a_out = nullptr;
				return E_NOINTERFACE;
			}
			AddRef();
			return S_OK;
		}
		STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&refs); }
		STDMETHODIMP_(ULONG) Release() override { return InterlockedDecrement(&refs); }  // on the stack
		STDMETHODIMP ActivateCompleted(IActivateAudioInterfaceAsyncOperation*) override
		{
			SetEvent(done);
			return S_OK;
		}

	private:
		LONG refs = 1;
	};

	void WriteWav(const char* a_path, const std::vector<float>& a_samples, const WAVEFORMATEX& a_format)
	{
		std::ofstream out(a_path, std::ios::binary);
		const auto put32 = [&](std::uint32_t a_v) { out.write(reinterpret_cast<const char*>(&a_v), 4); };
		const auto put16 = [&](std::uint16_t a_v) { out.write(reinterpret_cast<const char*>(&a_v), 2); };
		const std::uint32_t bytes = static_cast<std::uint32_t>(a_samples.size() * sizeof(float));
		out.write("RIFF", 4);
		put32(36 + bytes);
		out.write("WAVEfmt ", 8);
		put32(16);
		put16(3);  // IEEE float
		put16(a_format.nChannels);
		put32(a_format.nSamplesPerSec);
		put32(a_format.nSamplesPerSec * a_format.nChannels * 4);
		put16(static_cast<std::uint16_t>(a_format.nChannels * 4));
		put16(32);
		out.write("data", 4);
		put32(bytes);
		out.write(reinterpret_cast<const char*>(a_samples.data()), bytes);
	}
}

int main(int a_argc, char** a_argv)
{
	if (a_argc < 4) {
		std::fprintf(stderr, "usage: audio_capture <pid> <out.wav> <max seconds>\n");
		return 2;
	}
	const DWORD  pid = std::strtoul(a_argv[1], nullptr, 10);
	const char*  path = a_argv[2];
	const double limit = std::strtod(a_argv[3], nullptr);
	HANDLE process = OpenProcess(SYNCHRONIZE, FALSE, pid);
	if (!process) {
		std::fprintf(stderr, "no process %lu\n", pid);
		return 1;
	}
	CoInitializeEx(nullptr, COINIT_MULTITHREADED);

	AUDIOCLIENT_ACTIVATION_PARAMS params{};
	params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
	params.ProcessLoopbackParams.TargetProcessId = pid;
	params.ProcessLoopbackParams.ProcessLoopbackMode = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;
	PROPVARIANT activation{};
	activation.vt = VT_BLOB;
	activation.blob.cbSize = sizeof(params);
	activation.blob.pBlobData = reinterpret_cast<BYTE*>(&params);

	Activated                            handler;
	IActivateAudioInterfaceAsyncOperation* operation = nullptr;
	HRESULT hr = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, __uuidof(IAudioClient), &activation, &handler, &operation);
	if (FAILED(hr)) {
		std::fprintf(stderr, "ActivateAudioInterfaceAsync: 0x%08lx\n", hr);
		return 1;
	}
	WaitForSingleObject(handler.done, INFINITE);
	HRESULT   activated = E_FAIL;
	IUnknown* unknown = nullptr;
	operation->GetActivateResult(&activated, &unknown);
	operation->Release();
	if (FAILED(activated) || !unknown) {
		std::fprintf(stderr, "activation: 0x%08lx\n", activated);
		return 1;
	}
	IAudioClient* client = nullptr;
	unknown->QueryInterface(IID_PPV_ARGS(&client));
	unknown->Release();

	// Float, so a mix over full scale shows as samples over 1 (a 16-bit file would only clip).
	WAVEFORMATEX format{};
	format.wFormatTag = WAVE_FORMAT_IEEE_FLOAT;
	format.nChannels = 2;
	format.nSamplesPerSec = 48000;
	format.wBitsPerSample = 32;
	format.nBlockAlign = 8;
	format.nAvgBytesPerSec = 48000 * 8;
	hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 2000000, 0, &format, nullptr);
	if (FAILED(hr)) {
		std::fprintf(stderr, "Initialize: 0x%08lx\n", hr);
		return 1;
	}
	HANDLE ready = CreateEventW(nullptr, FALSE, FALSE, nullptr);
	client->SetEventHandle(ready);
	IAudioCaptureClient* capture = nullptr;
	client->GetService(IID_PPV_ARGS(&capture));

	std::vector<float> samples;
	samples.reserve(static_cast<std::size_t>(limit * 48000 * 2));
	LARGE_INTEGER qpcFrequency, qpcStart;
	QueryPerformanceFrequency(&qpcFrequency);
	SYSTEMTIME startTime;
	client->Start();
	QueryPerformanceCounter(&qpcStart);
	GetLocalTime(&startTime);
	const double start100ns = static_cast<double>(qpcStart.QuadPart) * 1.0e7 / static_cast<double>(qpcFrequency.QuadPart);
	std::size_t gaps = 0;

	for (;;) {
		LARGE_INTEGER now;
		QueryPerformanceCounter(&now);
		const double elapsed = static_cast<double>(now.QuadPart - qpcStart.QuadPart) / static_cast<double>(qpcFrequency.QuadPart);
		if (elapsed > limit || WaitForSingleObject(process, 0) == WAIT_OBJECT_0) {
			break;
		}
		WaitForSingleObject(ready, 100);
		UINT32 packet = 0;
		while (SUCCEEDED(capture->GetNextPacketSize(&packet)) && packet > 0) {
			BYTE*  data = nullptr;
			UINT32 frames = 0;
			DWORD  flags = 0;
			UINT64 qpc100ns = 0;
			if (FAILED(capture->GetBuffer(&data, &frames, &flags, nullptr, &qpc100ns))) {
				break;
			}
			// Where this packet starts on the file's clock: silence up to there (the process
			// sends nothing while it's quiet).
			const double at = (static_cast<double>(qpc100ns) - start100ns) / 1.0e7;
			const auto   want = static_cast<std::size_t>(std::max(0.0, at) * 48000.0) * 2;
			if (want > samples.size() + 2 * 48 * 5) {  // more than 5 ms behind
				samples.resize(want, 0.0f);
				++gaps;
			}
			if ((flags & AUDCLNT_BUFFERFLAGS_SILENT) || !data) {
				samples.resize(samples.size() + frames * 2, 0.0f);
			} else {
				const float* f = reinterpret_cast<const float*>(data);
				samples.insert(samples.end(), f, f + frames * 2);
			}
			capture->ReleaseBuffer(frames);
		}
	}
	client->Stop();
	WriteWav(path, samples, format);
	if (FILE* start = std::fopen((std::string(path) + ".start").c_str(), "w")) {
		std::fprintf(start, "%02d:%02d:%02d.%03d\n", startTime.wHour, startTime.wMinute, startTime.wSecond, startTime.wMilliseconds);
		std::fclose(start);
	}
	std::printf("%s: %.1f s, %zu silences filled\n", path, static_cast<double>(samples.size()) / 2.0 / 48000.0, gaps);
	capture->Release();
	client->Release();
	CloseHandle(process);
	CoUninitialize();
	return 0;
}
