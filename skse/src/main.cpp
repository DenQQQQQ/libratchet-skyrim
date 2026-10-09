// SPDX-License-Identifier: GPL-3.0-or-later
#include "AutoTest.h"
#include "Bolts.h"
#include "Camera.h"
#include "Clank.h"
#include "CoSave.h"
#include "Combat.h"
#include "Fx.h"
#include "Items.h"
#include "Jump.h"
#include "LoadScreens.h"
#include "MainMenu.h"
#include "Model.h"
#include "RatchetDll.h"
#include "Settings.h"
#include "Sounds.h"

SKSEPluginInfo(
	.Version = REL::Version{ PLUGIN_VERSION_MAJOR, PLUGIN_VERSION_MINOR, PLUGIN_VERSION_PATCH, 0 },
	.Name = PLUGIN_NAME,
	.Author = "riftapart-skyrim",
	// Independent: CommonLib picks the struct layout at run time (before and after 1.6.629).
	// Dependent tells SKSE the plugin only knows the pre-629 layouts, and SKSE disables it.
	.StructCompatibility = SKSE::StructCompatibility::Independent,
	.RuntimeCompatibility = SKSE::VersionIndependence::AddressLibrary)

namespace
{
	void OnMessage(SKSE::MessagingInterface::Message* a_msg)
	{
		switch (a_msg->type) {
		case SKSE::MessagingInterface::kDataLoaded:
			Jump::Install();
			Camera::Install(ConfigsDirectory());
			if (Settings::Get().combat) {
				Items::Install();  // (before Combat's: it asks whether his guns are Skyrim weapons)
			}
			Combat::Install();
			Bolts::Install();
			LoadScreens::Install();
			MainMenu::Install();
			AutoTest::Install();
			break;
		case SKSE::MessagingInterface::kPreLoadGame:
		case SKSE::MessagingInterface::kNewGame:
			Jump::Reset();
			Combat::Reset();
			Bolts::Reset();
			Model::Reset();
			Sounds::Reset();
			if (a_msg->type == SKSE::MessagingInterface::kNewGame) {
				Model::Preload();
				Fx::Preload();
			}
			break;
		case SKSE::MessagingInterface::kPostLoadGame:
			Jump::Reset();
			Combat::Reset();
			Bolts::Reset();
			Model::Reset();
			Sounds::Reset();
			// (Once a session, as the game loads: his models and the effects' templates.)
			Model::Preload();
			Fx::Preload();
			AutoTest::OnGameLoaded();
			break;
		default:
			break;
		}
	}
}

SKSEPluginLoad(const SKSE::LoadInterface* a_skse)
{
	SKSE::Init(a_skse, { .trampoline = true, .trampolineSize = 64 });

	const auto version = a_skse->RuntimeVersion();
	SKSE::log::info("{} {}.{}.{} on Skyrim {}", PLUGIN_NAME, PLUGIN_VERSION_MAJOR, PLUGIN_VERSION_MINOR, PLUGIN_VERSION_PATCH, version.string());

	const auto dir = PluginDirectory();
	auto& settings = Settings::Get();
	settings.Load(dir / L"RatchetMovement.ini");
	SKSE::log::info("settings: enabled {}, scale {}, configs '{}'", settings.enabled, settings.scale, settings.configsDir);
	if (!settings.enabled) {
		SKSE::log::info("off in the INI: Skyrim's own jump stays");
		return true;
	}

	if (!Ratchet::Load(dir)) {
		return true;  // logged; the game runs on without us
	}
	AutoTest::Load(dir);
	if (Model::Load(dir)) {
		SKSE::log::info("ratchet model: on");
		if (Clank::Load(dir)) {
			SKSE::log::info("clank: on");
		}
	}
	Sounds::Load(dir);
	CoSave::Register();
	const auto& e = Ratchet::Expected();
	SKSE::log::info("sandbox numbers at scale {}: tap apex {:.3f} m at {:.3f} s, full-hold apex {:.3f} m at {:.3f} s, air time {:.3f} s",
		settings.scale, e.tapApex, e.tapTime, e.fullApex, e.fullTime, e.fullAirTime);

	SKSE::GetMessagingInterface()->RegisterListener(OnMessage);
	return true;
}
