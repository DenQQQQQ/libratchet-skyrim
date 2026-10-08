// SPDX-License-Identifier: GPL-3.0-or-later
#include "TestWindow.h"

#include <d3d11.h>
#include <deque>
#include <dxgi.h>
#include <wincodec.h>

#pragma comment(lib, "windowscodecs.lib")

namespace TestWindow
{
	namespace
	{
		constexpr const char* kClassName = "Skyrim Special Edition";

		bool                  g_corner = true;
		RECT                  g_rect{};
		bool                  g_haveRect = false;
		std::filesystem::path g_folder;
		std::mutex            g_lock;
		std::deque<std::string> g_pending;  // shots asked for, not yet copied (one a frame, in order)
		std::uint64_t         g_frame = 0;       // frames presented
		std::uint64_t         g_frameCalls = 0;  // watched Map calls this frame

		// A copy of a finished frame on its way back from the GPU (read once it's done, never
		// waited for: the frame goes on meanwhile).
		struct Readback
		{
			ID3D11Texture2D* staging = nullptr;
			std::string      name;
			UINT             width = 0, height = 0;
			DXGI_FORMAT      format = DXGI_FORMAT_UNKNOWN;
			int              frames = 0;
		};
		std::optional<Readback> g_readback;

		// A third of the primary monitor's work area, in its bottom-right corner.
		RECT CornerRect()
		{
			RECT work{};
			SystemParametersInfoW(SPI_GETWORKAREA, 0, &work, 0);
			const LONG w = (work.right - work.left) / 3;
			const LONG h = w * 9 / 16;
			return { work.right - w, work.bottom - h, work.right, work.bottom };
		}

		using CreateWindowExA_t = HWND(WINAPI*)(DWORD, LPCSTR, LPCSTR, DWORD, int, int, int, int, HWND, HMENU, HINSTANCE, LPVOID);
		CreateWindowExA_t g_createWindowExA = nullptr;

		HWND WINAPI MyCreateWindowExA(DWORD a_exStyle, LPCSTR a_class, LPCSTR a_name, DWORD a_style, int a_x, int a_y, int a_w, int a_h,
			HWND a_parent, HMENU a_menu, HINSTANCE a_instance, LPVOID a_param)
		{
			const bool game = a_class && (reinterpret_cast<std::uintptr_t>(a_class) >> 16) != 0 && std::strcmp(a_class, kClassName) == 0;
			if (game && g_corner) {
				g_rect = CornerRect();
				g_haveRect = true;
				a_x = g_rect.left;
				a_y = g_rect.top;
				a_w = g_rect.right - g_rect.left;
				a_h = g_rect.bottom - g_rect.top;
				// Never the foreground window: the test plays itself, and the owner keeps typing.
				a_exStyle |= WS_EX_NOACTIVATE;
				SKSE::log::info("autotest: the game's window goes in the corner ({}x{})", a_w, a_h);
			}
			return g_createWindowExA(a_exStyle, a_class, a_name, a_style, a_x, a_y, a_w, a_h, a_parent, a_menu, a_instance, a_param);
		}

		HWND GameWindow()
		{
			auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
			return renderer ? reinterpret_cast<HWND>(renderer->GetRuntimeData().renderWindows[0].hWnd) : nullptr;
		}

		ID3D11DeviceContext* Context()
		{
			auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
			return renderer ? reinterpret_cast<ID3D11DeviceContext*>(renderer->GetRuntimeData().context) : nullptr;
		}

		// BGRA pixels to a PNG through Windows' own codecs (on a worker thread).
		void WritePng(std::filesystem::path a_path, UINT a_w, UINT a_h, std::vector<std::uint8_t> a_bgra)
		{
			const HRESULT co = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
			IWICImagingFactory*    factory = nullptr;
			IWICStream*            stream = nullptr;
			IWICBitmapEncoder*     encoder = nullptr;
			IWICBitmapFrameEncode* frame = nullptr;
			bool                   ok = SUCCEEDED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) &&
			          SUCCEEDED(factory->CreateStream(&stream)) && SUCCEEDED(stream->InitializeFromFilename(a_path.c_str(), GENERIC_WRITE)) &&
			          SUCCEEDED(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder)) &&
			          SUCCEEDED(encoder->Initialize(stream, WICBitmapEncoderNoCache)) && SUCCEEDED(encoder->CreateNewFrame(&frame, nullptr)) &&
			          SUCCEEDED(frame->Initialize(nullptr)) && SUCCEEDED(frame->SetSize(a_w, a_h));
			if (ok) {
				WICPixelFormatGUID fmt = GUID_WICPixelFormat32bppBGRA;
				ok = SUCCEEDED(frame->SetPixelFormat(&fmt)) &&
				     SUCCEEDED(frame->WritePixels(a_h, a_w * 4, static_cast<UINT>(a_bgra.size()), a_bgra.data())) && SUCCEEDED(frame->Commit()) &&
				     SUCCEEDED(encoder->Commit());
			}
			for (IUnknown* p : { static_cast<IUnknown*>(frame), static_cast<IUnknown*>(encoder), static_cast<IUnknown*>(stream), static_cast<IUnknown*>(factory) }) {
				if (p) {
					p->Release();
				}
			}
			if (SUCCEEDED(co)) {
				CoUninitialize();
			}
			SKSE::log::info("autotest: shot {}{}", a_path.string(), ok ? "" : " FAILED");
		}

		// Reads the copy back if the GPU has finished it; true once it's dealt with.
		bool TryReadback(ID3D11DeviceContext* a_ctx)
		{
			auto& rb = *g_readback;
			D3D11_MAPPED_SUBRESOURCE mapped{};
			const HRESULT hr = a_ctx->Map(rb.staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT, &mapped);
			if (hr == DXGI_ERROR_WAS_STILL_DRAWING && ++rb.frames < 120) {
				return false;
			}
			if (SUCCEEDED(hr)) {
				std::vector<std::uint8_t> bgra(static_cast<std::size_t>(rb.width) * rb.height * 4);
				const bool rgba = rb.format == DXGI_FORMAT_R8G8B8A8_UNORM || rb.format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
				for (UINT y = 0; y < rb.height; ++y) {
					const auto* src = static_cast<const std::uint8_t*>(mapped.pData) + static_cast<std::size_t>(y) * mapped.RowPitch;
					auto*       dst = bgra.data() + static_cast<std::size_t>(y) * rb.width * 4;
					for (UINT x = 0; x < rb.width; ++x) {
						dst[x * 4 + 0] = rgba ? src[x * 4 + 2] : src[x * 4 + 0];
						dst[x * 4 + 1] = src[x * 4 + 1];
						dst[x * 4 + 2] = rgba ? src[x * 4 + 0] : src[x * 4 + 2];
						dst[x * 4 + 3] = 255;
					}
				}
				a_ctx->Unmap(rb.staging, 0);
				std::error_code ec;
				std::filesystem::create_directories(g_folder, ec);
				std::thread(WritePng, g_folder / (rb.name + ".png"), rb.width, rb.height, std::move(bgra)).detach();
			} else {
				SKSE::log::warn("autotest: shot {} not read back ({:#x})", rb.name, static_cast<std::uint32_t>(hr));
			}
			rb.staging->Release();
			g_readback.reset();
			return true;
		}

		// IDXGISwapChain::Present (vtable slot 8): the finished frame is in the back buffer.
		using Present_t = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
		Present_t g_present = nullptr;

		HRESULT STDMETHODCALLTYPE MyPresent(IDXGISwapChain* a_this, UINT a_sync, UINT a_flags)
		{
			++g_frame;
			g_frameCalls = 0;
			auto* ctx = Context();
			if (ctx && g_readback) {
				TryReadback(ctx);
			}
			std::string name;
			if (ctx && !g_readback) {
				std::lock_guard l(g_lock);
				if (!g_pending.empty()) {
					name = std::move(g_pending.front());
					g_pending.pop_front();
				}
			}
			if (!name.empty() && !g_folder.empty()) {
				ID3D11Texture2D* back = nullptr;
				if (SUCCEEDED(a_this->GetBuffer(0, __uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&back)))) {
					D3D11_TEXTURE2D_DESC desc{};
					back->GetDesc(&desc);
					desc.Usage = D3D11_USAGE_STAGING;
					desc.BindFlags = 0;
					desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
					desc.MiscFlags = 0;
					desc.SampleDesc = { 1, 0 };
					ID3D11Device*    device = nullptr;
					ID3D11Texture2D* staging = nullptr;
					ctx->GetDevice(&device);
					if (device && SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &staging))) {
						ctx->CopyResource(staging, back);
						g_readback = Readback{ staging, name, desc.Width, desc.Height, desc.Format, 0 };
					}
					if (device) {
						device->Release();
					}
					back->Release();
				}
			}
			return g_present(a_this, a_sync, a_flags);
		}

		// Diagnosis of the renderer hang: every Map from the engine call site in the hung stacks
		// (SkyrimSE+0xe4ad24, which returns from d3d11 Map) is logged before it runs, so the last
		// line before a hang names the resource.
		using Map_t = HRESULT(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT, D3D11_MAP, UINT, D3D11_MAPPED_SUBRESOURCE*);
		Map_t                      g_map = nullptr;
		std::uintptr_t             g_watchSite = 0;
		std::atomic<std::uint64_t> g_siteCalls{ 0 };

		HRESULT STDMETHODCALLTYPE MyMap(ID3D11DeviceContext* a_this, ID3D11Resource* a_res, UINT a_sub, D3D11_MAP a_type, UINT a_flags,
			D3D11_MAPPED_SUBRESOURCE* a_out)
		{
			const auto ret = reinterpret_cast<std::uintptr_t>(_ReturnAddress());
			if (ret == g_watchSite && a_res) {
				const std::uint64_t n = ++g_siteCalls;
				++g_frameCalls;
				// A few lines, not a line per call: a runaway frame is millions of them.
				if (n <= 40 || n % 2000 == 0 || (g_frameCalls > 2000 && g_frameCalls % 1000000 == 1)) {
					D3D11_RESOURCE_DIMENSION dim{};
					a_res->GetType(&dim);
					std::string what = std::format("dimension {}", static_cast<int>(dim));
					if (dim == D3D11_RESOURCE_DIMENSION_BUFFER) {
						D3D11_BUFFER_DESC d{};
						static_cast<ID3D11Buffer*>(a_res)->GetDesc(&d);
						what = std::format("buffer {} bytes usage {} bind {:#x} cpu {:#x}", d.ByteWidth, static_cast<int>(d.Usage), d.BindFlags, d.CPUAccessFlags);
					}
					SKSE::log::info("map#{} frame {} (#{} this frame): {} {:p} type {} flags {:#x}", n, g_frame, g_frameCalls, what,
						static_cast<void*>(a_res), static_cast<int>(a_type), a_flags);
				}
			}
			return g_map(a_this, a_res, a_sub, a_type, a_flags, a_out);
		}

		void HookMap(ID3D11DeviceContext* a_ctx)
		{
			if (g_map || !a_ctx) {
				return;
			}
			g_watchSite = REL::Module::get().base() + 0xe4ad24;
			auto** vtbl = *reinterpret_cast<void***>(a_ctx);
			DWORD old = 0;
			if (VirtualProtect(&vtbl[14], sizeof(void*), PAGE_EXECUTE_READWRITE, &old)) {
				g_map = reinterpret_cast<Map_t>(vtbl[14]);
				vtbl[14] = reinterpret_cast<void*>(&MyMap);
				VirtualProtect(&vtbl[14], sizeof(void*), old, &old);
				SKSE::log::info("autotest: watching Map from SkyrimSE+0xe4ad24");
			}
		}

		void HookPresent()
		{
			auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
			auto* swapChain = renderer ? reinterpret_cast<IDXGISwapChain*>(renderer->GetRuntimeData().renderWindows[0].swapChain) : nullptr;
			if (!swapChain || g_present) {
				return;
			}
			auto** vtbl = *reinterpret_cast<void***>(swapChain);
			DWORD old = 0;
			if (VirtualProtect(&vtbl[8], sizeof(void*), PAGE_EXECUTE_READWRITE, &old)) {
				g_present = reinterpret_cast<Present_t>(vtbl[8]);
				vtbl[8] = reinterpret_cast<void*>(&MyPresent);
				VirtualProtect(&vtbl[8], sizeof(void*), old, &old);
				SKSE::log::info("autotest: screenshots on");
			}
		}
	}

	void Install(bool a_corner)
	{
		g_corner = a_corner;
		if (!g_corner) {
			return;
		}
		g_createWindowExA = reinterpret_cast<CreateWindowExA_t>(SKSE::PatchIAT(&MyCreateWindowExA, "user32.dll", "CreateWindowExA"));
		if (!g_createWindowExA) {
			SKSE::log::warn("autotest: couldn't hook CreateWindowExA; the window stays as Skyrim makes it");
		}
	}

	void OnDataLoaded(bool a_shots, bool a_watchMap)
	{
		if (g_corner) {
			if (!g_haveRect) {
				g_rect = CornerRect();
				g_haveRect = true;
			}
			Keep();
		}
		if (a_shots || a_watchMap) {
			HookPresent();
		}
		if (a_watchMap) {
			HookMap(Context());
		}
	}

	void Keep()
	{
		HWND hwnd = GameWindow();
		if (!g_corner || !hwnd || !g_haveRect) {
			return;
		}
		RECT r{};
		GetWindowRect(hwnd, &r);
		if (r.left != g_rect.left || r.top != g_rect.top || r.right != g_rect.right || r.bottom != g_rect.bottom) {
			SetWindowPos(hwnd, HWND_BOTTOM, g_rect.left, g_rect.top, g_rect.right - g_rect.left, g_rect.bottom - g_rect.top,
				SWP_NOACTIVATE | SWP_NOOWNERZORDER);
		}
	}

	void Shot(std::string_view a_name)
	{
		std::lock_guard l(g_lock);
		g_pending.emplace_back(a_name);
	}

	void SetFolder(const std::filesystem::path& a_folder) { g_folder = a_folder; }
}
