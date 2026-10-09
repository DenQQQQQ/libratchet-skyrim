// SPDX-License-Identifier: GPL-3.0-or-later
#include "MainMenu.h"

#include "Settings.h"

namespace MainMenu
{
	namespace
	{
		// The menu music (skse\tools\ratchet_menu_to_skyrim.py), under Data: its intro, then its
		// seamless loop over and over, as the game plays it.
		constexpr const char* kIntro = "Sound\\FX\\RatchetMovement\\menu\\title_intro.wav";
		constexpr const char* kLoop = "Sound\\FX\\RatchetMovement\\menu\\title_loop.wav";
		// It fades out over this long as the menu goes (seconds; ours: the intro fades in itself).
		constexpr float kFade = 1.0f;
		// How loud (ours: its peaks near Skyrim's own menu theme's; the game plays it 12 dB down
		// under its other sounds, none of which play here).
		constexpr float kVolume = 0.8f;

		std::mutex          g_lock;
		RE::BSSoundHandle   g_music;
		bool                g_introPlayed = false;
		bool                g_menuOpen = false;
		std::uint32_t       g_session = 0;  // each time the menu opens (the watcher of an old one stops)
		RE::BGSSoundCategory* g_category = nullptr;  // Skyrim's music
		float               g_categoryVolume = 1.0f;
		bool                g_muted = false;

		bool MusicInstalled()
		{
			const auto dir = PluginDirectory().parent_path().parent_path() / L"Sound" / L"FX" / L"RatchetMovement" / L"menu";
			std::error_code ec;
			return std::filesystem::exists(dir / L"title_intro.wav", ec) && std::filesystem::exists(dir / L"title_loop.wav", ec);
		}

		// Skyrim's music category: the one the audio menu calls Music (its name is the menu's
		// "$Music"; Skyrim.esm's 00071E64).
		RE::BGSSoundCategory* MusicCategory()
		{
			auto* data = RE::TESDataHandler::GetSingleton();
			if (!data) {
				return nullptr;
			}
			for (auto* category : data->GetFormArray<RE::BGSSoundCategory>()) {
				const char* name = category ? category->GetFullName() : nullptr;
				if (name && (_stricmp(name, "$Music") == 0 || _stricmp(name, "Music") == 0)) {
					return category;
				}
			}
			return RE::TESForm::LookupByID<RE::BGSSoundCategory>(0x00071E64);
		}

		// (Main thread.)
		void Start()
		{
			std::scoped_lock lock(g_lock);
			if (!g_menuOpen) {
				return;
			}
			if (g_category && !g_muted) {
				g_categoryVolume = g_category->GetStaticVolumeMultiplier();
				g_category->SetStaticVolumeMultiplier(0.0f);
				g_muted = true;
			}
			if (g_music.IsValid() && g_music.IsPlaying()) {
				return;
			}
			auto* audio = RE::BSAudioManager::GetSingleton();
			if (!audio) {
				return;
			}
			// The intro the first time the menu's up this session, the loop after.
			const char* file = g_introPlayed ? kLoop : kIntro;
			RE::BSResource::ID id;
			id.GenerateFromPath(file);
			audio->GetSoundHandleByFile(g_music, id, 0x1A, 128);
			if (!g_music.IsValid()) {
				SKSE::log::warn("main menu: Skyrim won't play {}", file);
				return;
			}
			g_music.SetVolume(std::clamp(kVolume * Settings::Get().soundVolume, 0.0f, 1.0f));
			g_music.Play();
			g_introPlayed = true;
		}

		// (Main thread.)
		void Stop()
		{
			std::scoped_lock lock(g_lock);
			if (g_music.IsValid()) {
				g_music.FadeOutAndRelease(static_cast<std::uint16_t>(kFade * 1000.0f));
			}
			g_music = RE::BSSoundHandle{};
			if (g_category && g_muted) {
				g_category->SetStaticVolumeMultiplier(g_categoryVolume);
				g_muted = false;
			}
		}

		struct MenuSink final : RE::BSTEventSink<RE::MenuOpenCloseEvent>
		{
			RE::BSEventNotifyControl ProcessEvent(const RE::MenuOpenCloseEvent* a_event, RE::BSTEventSource<RE::MenuOpenCloseEvent>*) override
			{
				if (!a_event || a_event->menuName != RE::MainMenu::MENU_NAME) {
					return RE::BSEventNotifyControl::kContinue;
				}
				std::uint32_t session = 0;
				{
					std::scoped_lock lock(g_lock);
					g_menuOpen = a_event->opening;
					session = ++g_session;
				}
				if (!a_event->opening) {
					SKSE::GetTaskInterface()->AddTask([] { Stop(); });
					return RE::BSEventNotifyControl::kContinue;
				}
				SKSE::log::info("main menu: Rift Apart's menu music");
				SKSE::GetTaskInterface()->AddTask([] { Start(); });
				// While the menu's up: the music again when it ends (it's a loop), and Skyrim's kept
				// quiet (its theme starts a moment after the menu).
				std::thread([session] {
					for (;;) {
						std::this_thread::sleep_for(std::chrono::milliseconds(250));
						{
							std::scoped_lock lock(g_lock);
							if (!g_menuOpen || g_session != session) {
								return;
							}
						}
						SKSE::GetTaskInterface()->AddTask([] { Start(); });
					}
				}).detach();
				return RE::BSEventNotifyControl::kContinue;
			}
		};
	}

	void Install()
	{
		if (!MusicInstalled()) {
			SKSE::log::info("main menu: no {} and {}: Skyrim's own music", kIntro, kLoop);
			return;
		}
		g_category = MusicCategory();
		SKSE::log::info("main menu: Rift Apart's menu music, Skyrim's music category {}",
			g_category ? std::format("{:08X} muted meanwhile", g_category->GetFormID()) : std::string("NOT FOUND (both may play)"));
		static MenuSink sink;
		RE::UI::GetSingleton()->AddEventSink<RE::MenuOpenCloseEvent>(&sink);
	}

	bool MusicPlaying()
	{
		std::scoped_lock lock(g_lock);
		return g_music.IsValid() && g_music.IsPlaying();
	}
}
