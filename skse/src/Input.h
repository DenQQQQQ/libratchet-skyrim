// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// The player's buttons, read from the devices each frame through Skyrim's own key bindings.
namespace Input
{
	// Logs what Jump is bound to (once the control map exists).
	void LogBindings();
	// Jump is held now, on any device it's bound to, and Skyrim lets the player jump.
	bool JumpHeld();
}
