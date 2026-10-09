// SPDX-License-Identifier: GPL-3.0-or-later
#include "Input.h"

#include "Settings.h"

namespace Input
{
	namespace
	{
		constexpr std::uint32_t kUnmapped = 0xFF;

		std::uint32_t MappedKey(std::string_view a_control, RE::INPUT_DEVICE a_device)
		{
			const auto* map = RE::ControlMap::GetSingleton();
			if (!map || a_control.empty()) {
				return kUnmapped;
			}
			return map->GetMappedKey(a_control, a_device);
		}

		// The keyboard's DirectInput state, read directly: CommonLib's BSWin32KeyboardDevice::IsPressed
		// drags in its device classes, which don't link.
		bool Down(const RE::BSWin32KeyboardDevice* a_keyboard, std::uint32_t a_key)
		{
			return a_keyboard && a_key < sizeof(a_keyboard->curState) && (a_keyboard->curState[a_key] & 0x80) != 0;
		}

		bool Down(const RE::BSInputDevice* a_device, std::uint32_t a_key)
		{
			return a_device && a_key != kUnmapped && a_device->IsPressed(a_key);
		}

		// Polled rather than taken from button events, so a release can't be missed while a menu
		// or a script had the controls.
		bool Held(std::string_view a_control)
		{
			auto* devices = RE::BSInputDeviceManager::GetSingleton();
			if (!devices || a_control.empty()) {
				return false;
			}
			return Down(devices->GetKeyboard(), MappedKey(a_control, RE::INPUT_DEVICE::kKeyboard)) ||
			       Down(devices->GetMouse(), MappedKey(a_control, RE::INPUT_DEVICE::kMouse)) ||
			       (devices->IsGamepadEnabled() && Down(devices->GetGamepad(), MappedKey(a_control, RE::INPUT_DEVICE::kGamepad)));
		}

		void LogBinding(std::string_view a_control, std::string_view a_use)
		{
			SKSE::log::info("{} ({}) is bound to keyboard {:#x}, mouse {:#x}, gamepad {:#x}", a_control, a_use,
				MappedKey(a_control, RE::INPUT_DEVICE::kKeyboard), MappedKey(a_control, RE::INPUT_DEVICE::kMouse),
				MappedKey(a_control, RE::INPUT_DEVICE::kGamepad));
		}
	}

	void LogBindings()
	{
		const auto& s = Settings::Get();
		LogBinding("Jump", "jump");
		if (!s.dashControl.empty()) {
			LogBinding(s.dashControl, "dash");
		}
		if (s.combat) {
			LogBinding(s.fireControl, "fire");
			LogBinding(s.aimControl, "aim");
			LogBinding(s.meleeControl, "wrench");
			LogBinding(s.throwControl, "wrench throw");
			LogBinding(s.pistolControl, "Burst Pistol");
			LogBinding(s.shatterbombControl, "Shatterbomb");
		}
	}

	namespace
	{
		std::optional<RE::NiPoint2> g_testStick;
		bool                        g_testStickWritten = false;
	}

	RE::NiPoint2 MoveStick()
	{
		auto* controls = RE::PlayerControls::GetSingleton();
		const auto* map = RE::ControlMap::GetSingleton();
		if (!controls) {
			return {};
		}
		if (g_testStick) {
			// Written where Skyrim keeps it, so its own locomotion moves him as if keys were held.
			controls->data.moveInputVec = *g_testStick;
			g_testStickWritten = true;
		} else if (g_testStickWritten) {
			controls->data.moveInputVec = {};
			g_testStickWritten = false;
		}
		if (map && !map->IsMovementControlsEnabled()) {
			return {};
		}
		RE::NiPoint2 v = controls->data.moveInputVec;
		// Skyrim's auto-move runs ahead with the stick at rest.
		if (controls->data.autoMove && v.x * v.x + v.y * v.y < 0.01f) {
			v = { 0.0f, 1.0f };
		}
		const float len = std::sqrt(v.x * v.x + v.y * v.y);
		if (len > 1.0f) {
			v.x /= len;
			v.y /= len;
		}
		// Walking (Skyrim's run toggle off): half the stick, which is a walk for the sim.
		if (!controls->data.running && !g_testStick) {
			v.x *= 0.5f;
			v.y *= 0.5f;
		}
		return v;
	}

	void SetTestStick(std::optional<RE::NiPoint2> a_stick) { g_testStick = a_stick; }

	bool JumpHeld()
	{
		const auto* map = RE::ControlMap::GetSingleton();
		return map && map->IsJumpingControlsEnabled() && Held("Jump");
	}

	bool ControlHeld(std::string_view a_control)
	{
		const auto* map = RE::ControlMap::GetSingleton();
		return map && map->IsFightingControlsEnabled() && Held(a_control);
	}

	bool SneakHeld()
	{
		const auto* map = RE::ControlMap::GetSingleton();
		return map && map->IsSneakingControlsEnabled() && Held("Sneak");
	}

	bool DashHeld()
	{
		const auto* map = RE::ControlMap::GetSingleton();
		return map && map->IsMovementControlsEnabled() && Held(Settings::Get().dashControl);
	}
}
