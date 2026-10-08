// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Test runs only (skse/test/ingame.ps1): Skyrim's window small, in a corner and behind the
// owner's windows (unless the run asks for the normal window), and screenshots of finished
// frames that never make the game wait for the GPU.
namespace TestWindow
{
	// At plugin load, before Skyrim makes its window.
	void Install(bool a_corner);
	// Once the renderer exists: puts the window in its corner, hooks Present for screenshots, and
	// (a_watchMap) logs the engine's Map calls from the site the renderer hang sits in.
	void OnDataLoaded(bool a_shots, bool a_watchMap);
	// Each frame: keeps the window where it was put (Skyrim and display mods move it).
	void Keep();
	// Saves a coming finished frame as <folder>\<name>.png.
	void Shot(std::string_view a_name);
	void SetFolder(const std::filesystem::path& a_folder);
}
