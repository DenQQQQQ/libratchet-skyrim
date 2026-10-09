// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// What the plugin keeps with a save, in SKSE's co-save: whether this game has had the guns
// (Items), and his bolts (Bolts). A new game, or a save from before the plugin, has neither.
namespace CoSave
{
	// At the plugin's load.
	void Register();
}
