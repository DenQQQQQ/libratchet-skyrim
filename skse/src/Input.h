// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// The player's buttons, read from the devices each frame through Skyrim's own key bindings.
namespace Input
{
	// Logs what Jump and the dash control are bound to (once the control map exists).
	void LogBindings();
	// Jump is held now, on any device it's bound to, and Skyrim lets the player jump.
	bool JumpHeld();
	// The dash control (INI sDashControl, Sprint by default) is held, and movement is allowed.
	bool DashHeld();
	// Sneak is held (the sim's crouch: it lets go of a ledge).
	bool SneakHeld();
	// A fighting control (by Skyrim's name, e.g. "Right Attack/Block") is held, and fighting is
	// allowed.
	bool ControlHeld(std::string_view a_control);
	// The movement stick (keys or left stick) as Skyrim reads it: x right, y forward, in the
	// camera's frame, length up to 1; zero when movement is switched off.
	RE::NiPoint2 MoveStick();
	// Tests: drive the movement stick as if the keys were held (Skyrim's own locomotion runs too);
	// nullopt gives it back.
	void SetTestStick(std::optional<RE::NiPoint2> a_stick);
}
