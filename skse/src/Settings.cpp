// SPDX-License-Identifier: GPL-3.0-or-later
#include "Settings.h"

namespace
{
	std::wstring ReadString(const std::filesystem::path& a_ini, const wchar_t* a_section, const wchar_t* a_key, const wchar_t* a_default)
	{
		wchar_t buf[1024]{};
		GetPrivateProfileStringW(a_section, a_key, a_default, buf, static_cast<DWORD>(std::size(buf)), a_ini.c_str());
		return buf;
	}

	bool ReadBool(const std::filesystem::path& a_ini, const wchar_t* a_section, const wchar_t* a_key, bool a_default)
	{
		return GetPrivateProfileIntW(a_section, a_key, a_default ? 1 : 0, a_ini.c_str()) != 0;
	}

	float ReadFloat(const std::filesystem::path& a_ini, const wchar_t* a_section, const wchar_t* a_key, float a_default)
	{
		const auto s = ReadString(a_ini, a_section, a_key, L"");
		if (s.empty()) {
			return a_default;
		}
		wchar_t* end = nullptr;
		const float v = std::wcstof(s.c_str(), &end);
		return end != s.c_str() && std::isfinite(v) ? v : a_default;
	}

	std::string Narrow(const std::wstring& a_s)
	{
		if (a_s.empty()) {
			return {};
		}
		const int n = WideCharToMultiByte(CP_UTF8, 0, a_s.c_str(), static_cast<int>(a_s.size()), nullptr, 0, nullptr, nullptr);
		std::string out(static_cast<std::size_t>(n), '\0');
		WideCharToMultiByte(CP_UTF8, 0, a_s.c_str(), static_cast<int>(a_s.size()), out.data(), n, nullptr, nullptr);
		return out;
	}
}

void Settings::Load(const std::filesystem::path& a_ini)
{
	if (!std::filesystem::exists(a_ini)) {
		SKSE::log::warn("{} not found; using defaults", a_ini.string());
		return;
	}
	enabled = ReadBool(a_ini, L"General", L"bEnabled", enabled);
	scale = ReadFloat(a_ini, L"General", L"fScale", scale);
	if (!(scale > 0.0f)) {
		scale = 1.55f;
	}
	configsDir = Narrow(ReadString(a_ini, L"General", L"sConfigsDir", L"RatchetMovement"));
	animations = ReadBool(a_ini, L"General", L"bAnimations", animations);
	ratchetModel = ReadBool(a_ini, L"General", L"bRatchet", ratchetModel);
	dashControl = Narrow(ReadString(a_ini, L"General", L"sDashControl", L"Sprint"));
	logJumps = ReadBool(a_ini, L"Debug", L"bLogJumps", logJumps);
	traceJumps = ReadBool(a_ini, L"Debug", L"bTraceJumps", traceJumps);
}

std::filesystem::path PluginDirectory()
{
	HMODULE self = nullptr;
	GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
		reinterpret_cast<LPCWSTR>(&PluginDirectory), &self);
	std::wstring path(MAX_PATH, L'\0');
	for (;;) {
		const DWORD n = GetModuleFileNameW(self, path.data(), static_cast<DWORD>(path.size()));
		if (n < path.size()) {
			path.resize(n);
			break;
		}
		path.resize(path.size() * 2);
	}
	return std::filesystem::path(path).parent_path();
}
