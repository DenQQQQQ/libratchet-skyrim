// SPDX-License-Identifier: GPL-3.0-or-later
#include "AutoTest.h"

#include "Model.h"
#include "RatchetDll.h"
#include "TestWindow.h"

namespace AutoTest
{
	namespace
	{
		struct Case
		{
			const char* name;
			float       hold;  // seconds the button is held; 0: one frame
		};
		constexpr Case kCases[] = {
			{ "tap", 0.0f },
			{ "short hold", 0.1f },
			{ "medium hold", 0.15f },
			{ "full hold", 0.5f },
		};

		// How close the game has to come to the sandbox. The hold is counted in frames (about
		// 100 fps here, 60 in the reference), which moves the apex by up to about 0.035 m.
		constexpr float kApexTolerance = 0.05f;      // metres
		constexpr float kApexTimeTolerance = 0.03f;  // seconds
		constexpr float kFloorTolerance = 0.05f;     // landing height on a flat floor, metres
		constexpr float kSettle = 1.5f;              // seconds stood still before each jump
		constexpr float kTimeout = 90.0f;            // seconds in game for the whole run

		struct State
		{
			bool        active = false;
			std::string start = "coc qasmoke";  // console command from the main menu, or "load"
			bool        flatGround = true;      // the landing must come back to the takeoff height
			bool        quit = true;
			float       holdEnd = 0.0f;         // seconds stood still after the last jump (for screenshots)
			float       held = 0.0f;
			bool        thirdPerson = false;    // the camera was put behind him
			bool        shots = true;           // screenshots (bShots)

			// bDiagnose: steps before the jumps that find what hangs the renderer, each logged
			// as it starts (the last one logged is the one that hung).
			bool        diagnose = false;
			int         phase = 0;
			float       phaseTime = 0.0f;
			int         shapeCount = 0;

			int   index = 0;
			float settle = 0.0f;
			float pressed = -1.0f;  // how long this case's press has been held; < 0 not pressed yet
			float elapsed = 0.0f;
			bool  launched = false;
			bool  landed = false;
			float healthBefore = 0.0f;
			float total = 0.0f;
			int   passed = 0;
			int   failed = 0;
			bool  done = false;
		};
		State s;

		float Health()
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			return player ? player->AsActorValueOwner()->GetActorValue(RE::ActorValue::kHealth) : 0.0f;
		}

		void Fail(std::string_view a_what)
		{
			++s.failed;
			SKSE::log::info("autotest: FAIL {}: {}", kCases[std::min<int>(s.index, static_cast<int>(std::size(kCases)) - 1)].name, a_what);
		}

		void Finish()
		{
			if (s.done) {
				return;
			}
			s.done = true;
			SKSE::log::info("autotest: done, {} passed, {} failed", s.passed, s.failed);
			if (s.quit) {
				RE::Main::GetSingleton()->quitGame = true;
			}
		}

		// The previous jump's landing didn't hurt (checked once he's stood still after it: Skyrim
		// applies fall damage after the landing frame).
		void CheckHealthAfter(int a_case)
		{
			const float now = Health();
			if (now < s.healthBefore - 0.01f) {
				++s.failed;
				SKSE::log::info("autotest: FAIL {}: the landing cost health ({:.1f} -> {:.1f})", kCases[a_case].name, s.healthBefore, now);
			}
		}

		void Phase(int a_next, std::string_view a_what)
		{
			s.phase = a_next;
			s.phaseTime = 0.0f;
			SKSE::log::info("autotest: phase {}: {}", a_next, a_what);
		}

		// One diagnostic step per frame; false once they're all done.
		bool Diagnose(float a_dt, bool a_grounded)
		{
			if (!s.diagnose) {
				return false;
			}
			s.phaseTime += a_dt;
			auto* camera = RE::PlayerCamera::GetSingleton();
			const int shapesFrom = 10;
			switch (s.phase) {
			case 0:
				if (a_grounded && s.phaseTime > 1.5f) {
					if (camera) {
						camera->ForceThirdPerson();
					}
					Phase(1, "third person, no Ratchet yet");
				}
				return true;
			case 1:
				if (s.phaseTime > 2.0f) {
					Model::SetAttachAllowed(true);
					Model::ShowShapes(-2);
					Phase(2, "Ratchet attached, every shape hidden");
				}
				return true;
			case 2:
				if (s.phaseTime > 1.5f) {
					const auto names = Model::ShapeNames();
					s.shapeCount = static_cast<int>(names.size());
					if (s.shapeCount == 0) {
						Phase(50, "no shapes found");
					} else {
						Model::ShowShapes(0);
						Phase(shapesFrom, std::format("only shape 0 ({})", names[0]));
					}
				}
				return true;
			case 50:
				if (s.phaseTime > 2.0f) {
					if (camera) {
						camera->ForceFirstPerson();
					}
					Phase(51, "first person");
				}
				return true;
			case 51:
				if (s.phaseTime > 1.0f) {
					if (camera) {
						camera->ForceThirdPerson();
					}
					Phase(52, "back to third person");
				}
				return true;
			case 52:
				if (s.phaseTime > 2.0f) {
					s.diagnose = false;
					SKSE::log::info("autotest: diagnosis done");
				}
				return true;
			default:
				if (s.phase >= shapesFrom && s.phase < shapesFrom + s.shapeCount && s.phaseTime > 0.7f) {
					const int next = s.phase - shapesFrom + 1;
					if (next < s.shapeCount) {
						Model::ShowShapes(next);
						Phase(shapesFrom + next, std::format("only shape {} ({})", next, Model::ShapeNames()[next]));
					} else {
						Model::ShowShapes(-1);
						TestWindow::Shot("all_shapes");
						Phase(50, "all shapes");
					}
				}
				return true;
			}
		}

		void NextCase()
		{
			s.pressed = -1.0f;
			s.settle = 0.0f;
			s.launched = false;
			s.landed = false;
			++s.index;
		}

		void RunConsole(const std::string& a_command)
		{
			auto* factory = RE::IFormFactory::GetConcreteFormFactoryByType<RE::Script>();
			auto* script = factory ? factory->Create() : nullptr;
			if (!script) {
				SKSE::log::error("autotest: can't run '{}'", a_command);
				return;
			}
			script->SetCommand(a_command);
			script->CompileAndRun(nullptr);
			delete script;
		}

		void SetPref(const char* a_name, bool a_bool, float a_float, bool a_isFloat)
		{
			RE::Setting* setting = nullptr;
			if (auto* prefs = RE::INIPrefSettingCollection::GetSingleton()) {
				setting = prefs->GetSetting(a_name);
			}
			if (!setting) {
				if (auto* ini = RE::INISettingCollection::GetSingleton()) {
					setting = ini->GetSetting(a_name);
				}
			}
			if (!setting) {
				SKSE::log::warn("autotest: no setting {}", a_name);
				return;
			}
			if (a_isFloat) {
				setting->data.f = a_float;
			} else {
				setting->data.b = a_bool;
			}
		}

		struct MainMenuSink : RE::BSTEventSink<RE::MenuOpenCloseEvent>
		{
			RE::BSEventNotifyControl ProcessEvent(const RE::MenuOpenCloseEvent* a_event, RE::BSTEventSource<RE::MenuOpenCloseEvent>*) override
			{
				if (!a_event || !a_event->opening || a_event->menuName != RE::MainMenu::MENU_NAME || started) {
					return RE::BSEventNotifyControl::kContinue;
				}
				started = true;
				// A moment for the menu to settle before leaving it.
				std::thread([] {
					std::this_thread::sleep_for(2s);
					SKSE::GetTaskInterface()->AddTask([] {
						if (s.start == "load") {
							SKSE::log::info("autotest: loading the most recent save");
							RE::BGSSaveLoadManager::GetSingleton()->LoadMostRecentSaveGame();
						} else {
							SKSE::log::info("autotest: {}", s.start);
							RunConsole(s.start);
						}
					});
				}).detach();
				return RE::BSEventNotifyControl::kContinue;
			}
			bool started = false;
		};
	}

	void Load(const std::filesystem::path& a_pluginDir)
	{
		const auto trigger = a_pluginDir / L"RatchetMovement" / L"autotest.ini";
		std::error_code ec;
		if (!std::filesystem::exists(trigger, ec)) {
			return;
		}
		wchar_t buf[256]{};
		GetPrivateProfileStringW(L"AutoTest", L"sStart", L"coc qasmoke", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.start.clear();
		for (const wchar_t* c = buf; *c; ++c) {
			s.start.push_back(static_cast<char>(*c));  // console commands are ASCII
		}
		s.flatGround = GetPrivateProfileIntW(L"AutoTest", L"bFlatGround", 1, trigger.c_str()) != 0;
		s.quit = GetPrivateProfileIntW(L"AutoTest", L"bQuit", 1, trigger.c_str()) != 0;
		GetPrivateProfileStringW(L"AutoTest", L"fHoldEnd", L"0", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.holdEnd = std::max(0.0f, std::wcstof(buf, nullptr));
		s.diagnose = GetPrivateProfileIntW(L"AutoTest", L"bDiagnose", 0, trigger.c_str()) != 0;
		if (s.diagnose) {
			Model::SetAttachAllowed(false);
		}
		// Screenshots go where the runner says (default: next to the SKSE logs).
		GetPrivateProfileStringW(L"AutoTest", L"sShots", L"", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		std::filesystem::path shots = buf;
		if (shots.empty()) {
			shots = SKSE::log::log_directory().value_or(a_pluginDir) / L"RatchetMovement_shots";
		}
		TestWindow::SetFolder(shots);
		s.shots = GetPrivateProfileIntW(L"AutoTest", L"bShots", 1, trigger.c_str()) != 0;
		TestWindow::Install(GetPrivateProfileIntW(L"AutoTest", L"bCorner", 1, trigger.c_str()) != 0);
		// One run per trigger: the next start is a normal game.
		std::filesystem::remove(trigger, ec);
		s.active = true;
		SKSE::log::info("autotest: on (start '{}', flat ground {}, quit {}){}", s.start, s.flatGround, s.quit,
			ec ? " - couldn't delete the trigger file!" : "");
	}

	bool Active() { return s.active; }

	void Install()
	{
		if (!s.active) {
			return;
		}
		// No timed autosaves during a test run (in memory only; the INI files aren't touched).
		// The bSaveOn* switches aren't in either INI collection; `coc` from the main menu saves
		// nothing anyway (checked: no new save after a run).
		SetPref("fAutosaveEveryXMins:SaveGame", false, 1.0e6f, true);
		TestWindow::OnDataLoaded(s.shots);
		static MainMenuSink sink;
		RE::UI::GetSingleton()->AddEventSink<RE::MenuOpenCloseEvent>(&sink);
	}

	bool JumpButton(float a_dt, bool a_grounded, bool a_inJump)
	{
		if (!s.active || s.done) {
			return false;
		}
		TestWindow::Keep();
		if (Diagnose(a_dt, a_grounded && !a_inJump)) {
			return false;
		}
		s.total += a_dt;
		if (s.total > kTimeout + s.holdEnd) {
			Fail(std::format("the run took over {:.0f} s", kTimeout));
			Finish();
			return false;
		}
		if (s.index >= static_cast<int>(std::size(kCases))) {
			// Stand still a moment, so the last landing's damage (if any) has been dealt.
			s.settle = a_grounded && !a_inJump ? s.settle + a_dt : 0.0f;
			if (s.settle >= kSettle && s.held == 0.0f) {
				CheckHealthAfter(s.index - 1);
				TestWindow::Shot("end");
				SKSE::log::info("autotest: holding {:.0f} s", s.holdEnd);
			}
			if (s.settle >= kSettle) {
				s.held += a_dt;
				if (s.held >= s.holdEnd) {
					Finish();
				}
			}
			return false;
		}
		const Case& c = kCases[s.index];

		if (s.pressed < 0.0f) {
			s.settle = a_grounded && !a_inJump ? s.settle + a_dt : 0.0f;
			// The camera behind him (coc starts in first person, where his model isn't drawn).
			if (!s.thirdPerson && s.settle > 0.0f) {
				s.thirdPerson = true;
				if (auto* camera = RE::PlayerCamera::GetSingleton()) {
					camera->ForceThirdPerson();
					SKSE::log::info("autotest: third person");
				}
			}
			if (s.settle < kSettle) {
				return false;
			}
			if (s.index == 0) {
				TestWindow::Shot("idle");
			}
			if (s.index > 0) {
				CheckHealthAfter(s.index - 1);
			}
			s.healthBefore = Health();
			s.pressed = 0.0f;
			s.elapsed = 0.0f;
			SKSE::log::info("autotest: {} (the button held {:.2f} s)", c.name, c.hold);
		}

		s.elapsed += a_dt;
		const bool hold = s.pressed == 0.0f || s.pressed < c.hold;
		s.pressed += a_dt;
		if (hold) {
			return true;
		}
		if (s.landed || (!s.launched && a_grounded && !a_inJump)) {
			if (!s.launched) {
				Fail("didn't take off");
			}
			NextCase();
		} else if (s.elapsed > 5.0f) {
			Fail("hasn't landed after 5 s");
			NextCase();
		}
		return false;
	}

	void OnTakeoff()
	{
		if (s.active) {
			s.launched = true;
		}
	}

	void OnApex()
	{
		if (s.active && !s.done && s.index < static_cast<int>(std::size(kCases))) {
			std::string name = kCases[s.index].name;
			std::ranges::replace(name, ' ', '_');
			TestWindow::Shot(name + "_apex");
		}
	}

	void OnLanded(const Jump& a_jump)
	{
		if (!s.active || s.done || s.index >= static_cast<int>(std::size(kCases))) {
			return;
		}
		s.landed = true;
		const auto expected = Ratchet::MeasureFlatJump(1.0f / 60.0f, a_jump.hold);
		std::string bad;
		if (std::abs(a_jump.apex - expected.fullApex) > kApexTolerance) {
			bad += " apex";
		}
		if (std::abs(a_jump.apexTime - expected.fullTime) > kApexTimeTolerance) {
			bad += " apex-time";
		}
		if (s.flatGround && std::abs(a_jump.landDz) > kFloorTolerance) {
			bad += " landing-height";
		}
		if (bad.empty()) {
			++s.passed;
		} else {
			++s.failed;
		}
		SKSE::log::info("autotest: {} {}: apex {:.3f} m (sandbox {:.3f}) at {:.3f} s ({:.3f}) for a {:.3f} s hold, landed {:+.3f} m after {:.3f} s (sandbox {:.3f}){}",
			bad.empty() ? "PASS" : "FAIL", kCases[s.index].name, a_jump.apex, expected.fullApex, a_jump.apexTime, expected.fullTime,
			a_jump.hold, a_jump.landDz, a_jump.airTime, expected.fullAirTime, bad.empty() ? "" : " - off:" + bad);
	}

	void OnHandedBack(const char* a_why)
	{
		if (!s.active || s.done) {
			return;
		}
		Fail(std::format("handed back to Skyrim mid-jump ({})", a_why));
		s.landed = true;
		if (std::string_view(a_why) == "dead") {
			Finish();
		}
	}
}
