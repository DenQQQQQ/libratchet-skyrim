// SPDX-License-Identifier: GPL-3.0-or-later
#include "Input.h"

namespace Input
{
	namespace
	{
		constexpr std::uint32_t kUnmapped = 0xFF;

		std::uint32_t MappedKey(RE::INPUT_DEVICE a_device)
		{
			const auto* map = RE::ControlMap::GetSingleton();
			const auto* events = RE::UserEvents::GetSingleton();
			if (!map || !events) {
				return kUnmapped;
			}
			return map->GetMappedKey(events->jump, a_device);
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
	}

	void LogBindings()
	{
		SKSE::log::info("Jump is bound to keyboard {:#x}, mouse {:#x}, gamepad {:#x}",
			MappedKey(RE::INPUT_DEVICE::kKeyboard), MappedKey(RE::INPUT_DEVICE::kMouse), MappedKey(RE::INPUT_DEVICE::kGamepad));
	}

	bool JumpHeld()
	{
		const auto* map = RE::ControlMap::GetSingleton();
		auto*       devices = RE::BSInputDeviceManager::GetSingleton();
		if (!map || !devices || !map->IsJumpingControlsEnabled()) {
			return false;
		}
		// Polled rather than taken from button events, so a release can't be missed while a
		// menu or a script had the controls.
		return Down(devices->GetKeyboard(), MappedKey(RE::INPUT_DEVICE::kKeyboard)) ||
		       Down(devices->GetMouse(), MappedKey(RE::INPUT_DEVICE::kMouse)) ||
		       (devices->IsGamepadEnabled() && Down(devices->GetGamepad(), MappedKey(RE::INPUT_DEVICE::kGamepad)));
	}
}
