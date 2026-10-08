// SPDX-License-Identifier: GPL-3.0-or-later
#include "AutoTest.h"

#include "Combat.h"
#include "Input.h"
#include "Model.h"
#include "Settings.h"
#include "Physics.h"
#include "RatchetDll.h"
#include "TestWindow.h"

namespace AutoTest
{
	namespace
	{
		struct Case
		{
			const char* name;
			float       hold;           // seconds the jump button is held; 0: one frame; < 0: no jump
			float       run = 0.0f;     // seconds of stick forward before the press (a run-up)
			bool        airStick = false;  // the stick forward from the press on
			float       again = 0.0f;   // seconds after the first press that it's pressed again (0: not)
			float       againHold = 0.0f;  // and held (until the landing at most)
			float       dashAt = -1.0f; // seconds after the start that the dash is pressed (one frame); < 0: not
			// A fight: it lasts this long (s), against a target standing targetAt metres ahead,
			// which then has to have lost at least minDamage health (× fDamageMult), from at
			// least minHits wrench hits.
			float       duration = 0.0f;
			float       targetAt = 0.0f;
			float       melee[3] = { -1.0f, -1.0f, -1.0f };  // wrench presses (one frame each), s from the start
			float       fireFrom = -1.0f;  // the trigger held from here to fireTo (s)
			float       fireTo = -1.0f;
			bool        aim = false;       // and the aim button with it
			float       minDamage = 0.0f;
			int         minHits = 0;
			float       throwAt = -1.0f;   // the wrench thrown (one frame), s from the start
		};
		// At Dragonborn scale with the owner's configs (the sandbox, flat floor): a press again
		// at 0.4 s double-jumps at 0.42 s (peak 4.34 m), and held on he glides from 0.93 s. A
		// ground dash goes 10.9 m in 0.53 s; an air dash flies flat at 10.85 m/s for 0.6 s. The
		// wrench's swings do 20 each (wpn_wrench_melee.json), the Hyper-Strike 50, a Burst Pistol
		// shot 11.36 at 6 shots/s from the hip and 3 aimed.
		constexpr Case kCases[] = {
			{ "tap", 0.0f },
			{ "short hold", 0.1f },
			{ "medium hold", 0.15f },
			{ "full hold", 0.5f },
			{ "standing jump steered forward", 0.5f, 0.0f, true },
			{ "running jump", 0.5f, 0.8f, true },
			{ "double jump", 0.3f, 0.0f, false, 0.4f, 0.1f },
			{ "glide", 0.3f, 0.0f, false, 0.4f, 3.0f },
			{ "dash", -1.0f, 0.0f, true, 0.0f, 0.0f, 0.0f },
			{ "air dash", 0.3f, 0.0f, true, 0.0f, 0.0f, 0.35f },
			{ .name = "wrench combo", .hold = -1.0f, .duration = 2.5f, .targetAt = 1.8f, .melee = { 0.0f, 0.2f, 0.45f }, .minDamage = 55.0f, .minHits = 3 },
			{ .name = "hyper-strike", .hold = 0.3f, .duration = 2.5f, .targetAt = 2.0f, .melee = { 0.35f, -1.0f, -1.0f }, .minDamage = 45.0f, .minHits = 1 },
			{ .name = "burst pistol", .hold = -1.0f, .duration = 2.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 1.0f, .minDamage = 30.0f },
			{ .name = "burst pistol aimed", .hold = -1.0f, .duration = 2.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 1.0f, .aim = true, .minDamage = 20.0f },
			{ .name = "wrench throw", .hold = -1.0f, .duration = 2.5f, .targetAt = 8.0f, .minDamage = 25.0f, .minHits = 1, .throwAt = 0.0f },
		};

		// How close the game has to come to the sandbox replaying the same frames: what differs is
		// Skyrim moving him (its collision, a frame of lag in reading his height).
		constexpr float kApexTolerance = 0.03f;      // metres
		// Times: this much, plus the frame they were seen in (a slow frame, 50 ms at times in
		// Whiterun, shows the apex that much later).
		constexpr float kApexTimeTolerance = 0.015f; // seconds
		constexpr float kAirTimeTolerance = 0.02f;   // seconds
		// Across the ground: a frame of his speed either way at the landing.
		constexpr float kDistanceTolerance = 0.2f;   // metres
		// A ground dash ends on the sim's own clock, so only the ground (slopes, bumps) can
		// make it go less far than the replay.
		constexpr float kDashTolerance = 0.25f;      // metres
		constexpr float kFloorTolerance = 0.05f;     // landing height on a flat floor, metres
		constexpr float kSettle = 1.5f;              // seconds stood still before each jump
		constexpr float kTimeout = 90.0f;            // seconds in game for the whole run

		struct State
		{
			bool        active = false;
			std::string start = "coc Whiterun";  // where to test: "load" (the most recent save) or a coc command
			std::string then;                    // the coc to run once the save has loaded
			bool        flatGround = false;      // the landing must come back to the takeoff height
			bool        quit = true;
			float       holdEnd = 0.0f;         // seconds stood still after the last jump (for screenshots)
			float       held = 0.0f;
			bool        thirdPerson = false;    // the camera was put behind him
			bool        shots = true;           // screenshots (bShots)
			bool        watchMap = false;       // log the engine's Map calls (bWatchMap)
			float       cameraYaw = 0.0f;       // the camera's turn round him from behind (fCameraYaw)

			// bDiagnose: steps before the jumps that find what hangs the renderer, each logged
			// as it starts (the last one logged is the one that hung).
			bool        diagnose = false;
			int         phase = 0;
			float       phaseTime = 0.0f;
			int         shapeCount = 0;

			int   index = 0;
			float settle = 0.0f;
			float pressed = -1.0f;  // since this case's start (its first press); < 0 not started yet
			bool  dashPressed = false;
			float running = 0.0f;   // this case's run-up so far
			// Every case starts where the first one did, facing the same way (runs and steered
			// jumps would otherwise carry him into the room's furniture).
			std::optional<RE::NiPoint3> startPos;
			float                       startHeading = 0.0f;
			bool                        returnPending = false;
			float elapsed = 0.0f;
			bool  launched = false;
			bool  landed = false;
			std::vector<std::string> shotsThisCase;  // moments already shot (a second apex gets "2")
			// The fights' target: a cow (EncCow) with its AI off and plenty of health.
			RE::ActorHandle target;
			bool            targetPlaced = false;
			float           targetHealth = 0.0f;  // as the fight began
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

		// The camera behind him, not right at his head: forced third person starts at the closest
		// zoom.
		void ThirdPerson()
		{
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!camera) {
				return;
			}
			camera->ForceThirdPerson();
			auto& state = camera->GetRuntimeData().cameraStates[RE::CameraStates::kThirdPerson];
			if (auto* tps = static_cast<RE::ThirdPersonState*>(state.get())) {
				tps->targetZoomOffset = 0.5f;
				tps->currentZoomOffset = 0.5f;
				// Round in front of him at three quarters (sCameraYaw radians from behind), to see him.
				tps->freeRotation.x = s.cameraYaw;
				tps->freeRotationEnabled = s.cameraYaw != 0.0f;
			}
		}

		// The heading with the most room for the running and steered jumps: clear ahead at chest
		// height and with headroom along the way (a full jump peaks 2.3 m up at his size).
		float ClearHeading(const RE::NiPoint3& a_feet, float a_current)
		{
			const float units = RE::bhkWorld::GetWorldScaleInverse();
			constexpr float kAhead = 18.0f;  // metres: an early air dash carries him about 15 m
			float best = a_current;
			float bestRoom = -1.0f;
			for (int i = 0; i < 16; ++i) {
				const float h = a_current + static_cast<float>(i) * (2.0f * 3.14159265f / 16.0f);
				const RE::NiPoint3 dir{ std::sin(h), std::cos(h), 0.0f };
				const RE::NiPoint3 chest = a_feet + RE::NiPoint3{ 0.0f, 0.0f, 1.0f * units };
				const auto wall = Physics::Ray(chest, chest + dir * (kAhead * units));
				float room = wall ? wall->fraction * kAhead : kAhead;
				for (float d = 1.0f; d < room; d += 1.0f) {
					const RE::NiPoint3 p = a_feet + dir * (d * units);
					if (Physics::Ray(p + RE::NiPoint3{ 0.0f, 0.0f, 0.3f * units }, p + RE::NiPoint3{ 0.0f, 0.0f, 4.3f * units })) {
						room = d;  // a ceiling under 4.3 m
						break;
					}
					// No low wall, step up or drop on the way (the sandbox replays the jump onto a
					// floor at the landing's height, so a gentle slope is fine).
					const auto ground = Physics::GroundDistance(p + RE::NiPoint3{ 0.0f, 0.0f, 1.0f * units }, 3.0f);
					if (!ground || *ground - 1.0f < -0.5f || *ground - 1.0f > 1.5f) {
						room = d;
						break;
					}
				}
				if (room > bestRoom + 0.5f) {
					bestRoom = room;
					best = h;
				}
			}
			SKSE::log::info("autotest: running room {:.1f} m that way", bestRoom);
			return best;
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
					ThirdPerson();
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
					ThirdPerson();
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
			Input::SetTestStick(std::nullopt);
			Combat::SetTestButtons(std::nullopt);
			Combat::SetTestAim(std::nullopt);
			s.returnPending = true;
			s.running = 0.0f;
			s.pressed = -1.0f;
			s.settle = 0.0f;
			s.launched = false;
			s.landed = false;
			s.dashPressed = false;
			s.targetPlaced = false;
			s.shotsThisCase.clear();
			++s.index;
		}

		float Units() { return RE::bhkWorld::GetWorldScaleInverse(); }

		RE::Actor* Target() { return s.target.get().get(); }

		float TargetHealth()
		{
			auto* target = Target();
			return target ? target->AsActorValueOwner()->GetActorValue(RE::ActorValue::kHealth) : 0.0f;
		}

		// The fight's target, a_metres ahead of the start, its AI off (it stands still) and
		// with plenty of health (it lasts every fight). Placed while he settles, so it's loaded
		// when the fight starts.
		void PlaceTarget(float a_metres)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || !s.startPos) {
				return;
			}
			RE::Actor* target = Target();
			if (!target || target->IsDead()) {
				auto* cow = RE::TESForm::LookupByID<RE::TESNPC>(0x00023A90);  // EncCow
				const auto placed = cow ? player->PlaceObjectAtMe(cow, false) : nullptr;
				target = placed ? placed->As<RE::Actor>() : nullptr;
				if (!target) {
					SKSE::log::info("autotest: couldn't place the target");
					return;
				}
				s.target = target->GetHandle();
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			target->SetPosition(*s.startPos + ahead * (a_metres * Units()) + RE::NiPoint3{ 0.0f, 0.0f, 0.2f * Units() }, true);
			target->EnableAI(false);
			auto* av = target->AsActorValueOwner();
			av->SetBaseActorValue(RE::ActorValue::kHealth, 1000.0f);
			av->RestoreActorValue(RE::ActorValue::kHealth, 10000.0f);
		}

		// What Skyrim multiplies the player's damage by at the save's difficulty (Master: 0.5).
		float DifficultyMult()
		{
			static constexpr const char* kSettings[] = { "fDiffMultHPByPCVE", "fDiffMultHPByPCE", "fDiffMultHPByPCN",
				"fDiffMultHPByPCH", "fDiffMultHPByPCVH", "fDiffMultHPByPCL" };
			auto* player = RE::PlayerCharacter::GetSingleton();
			const int difficulty = player ? std::clamp<int>(player->GetGameStatsData().difficulty, 0, 5) : 2;
			auto* settings = RE::GameSettingCollection::GetSingleton();
			auto* setting = settings ? settings->GetSetting(kSettings[difficulty]) : nullptr;
			return setting ? setting->GetFloat() : 1.0f;
		}

		// A fight is over: what the target lost against what it should have.
		void JudgeFight(const Case& a_case)
		{
			const float lost = s.targetHealth - TargetHealth();
			const Combat::Tally tally = Combat::TakeTally();
			// Skyrim's difficulty scales the player's damage, Ratchet's too.
			const float minDamage = a_case.minDamage * Settings::Get().damageMult * DifficultyMult();
			std::string bad;
			if (!Target()) {
				bad += " no-target";
			}
			if (lost < minDamage) {
				bad += " damage";
			}
			if (tally.hits < a_case.minHits) {
				bad += " hits";
			}
			if (bad.empty()) {
				++s.passed;
			} else {
				++s.failed;
			}
			SKSE::log::info("autotest: {} {}: the target lost {:.1f} health (at least {:.1f} at difficulty x{:.2f}), {} wrench hits (at least {}), {} shots, {} throws{}",
				bad.empty() ? "PASS" : "FAIL", a_case.name, lost, minDamage, DifficultyMult(), tally.hits, a_case.minHits, tally.shots,
				tally.throws, bad.empty() ? "" : " - off:" + bad);
			if (a_case.throwAt >= 0.0f && Combat::ThrownWrench()) {
				SKSE::log::info("autotest: {}: the wrench is still out after {:.1f} s", a_case.name, a_case.duration);
			}
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
				// A new game asks questions (AE's survival mode, other mods' setup): closed
				// unanswered, so the test plays on.
				if (a_event && a_event->opening && a_event->menuName == RE::MessageBoxMenu::MENU_NAME && !s.done) {
					SKSE::log::info("autotest: closing a message box");
					SKSE::GetTaskInterface()->AddTask([] {
						RE::UIMessageQueue::GetSingleton()->AddMessage(RE::MessageBoxMenu::MENU_NAME, RE::UI_MESSAGE_TYPE::kHide, nullptr);
					});
					return RE::BSEventNotifyControl::kContinue;
				}
				if (!a_event || !a_event->opening || a_event->menuName != RE::MainMenu::MENU_NAME || started) {
					return RE::BSEventNotifyControl::kContinue;
				}
				started = true;
				// A moment for the menu to settle before leaving it.
				std::thread([] {
					std::this_thread::sleep_for(2s);
					SKSE::GetTaskInterface()->AddTask([] {
						// A save, not a new game: a new game (coc from the main menu) asks AE's
						// survival-mode question, which blocks the run. Then, if asked, coc to the
						// test place from the loaded game.
						auto* saves = RE::BGSSaveLoadManager::GetSingleton();
						saves->PopulateSaveList();
						const bool loaded = saves->LoadMostRecentSaveGame();
						SKSE::log::info("autotest: loading the most recent save: {}", loaded ? "ok" : "FAILED");
						if (!loaded && !s.then.empty()) {
							SKSE::log::info("autotest: {} from the main menu instead", s.then);
							RunConsole(s.then);
							s.then.clear();
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
		GetPrivateProfileStringW(L"AutoTest", L"sStart", L"coc Whiterun", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.start.clear();
		for (const wchar_t* c = buf; *c; ++c) {
			s.start.push_back(static_cast<char>(*c));  // console commands are ASCII
		}
		s.then = s.start == "load" ? "" : s.start;
		s.flatGround = GetPrivateProfileIntW(L"AutoTest", L"bFlatGround", 0, trigger.c_str()) != 0;
		s.quit = GetPrivateProfileIntW(L"AutoTest", L"bQuit", 1, trigger.c_str()) != 0;
		GetPrivateProfileStringW(L"AutoTest", L"fHoldEnd", L"0", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.holdEnd = std::max(0.0f, std::wcstof(buf, nullptr));
		s.diagnose = GetPrivateProfileIntW(L"AutoTest", L"bDiagnose", 0, trigger.c_str()) != 0;
		if (GetPrivateProfileIntW(L"AutoTest", L"bNoShadows", 0, trigger.c_str()) != 0) {
			Model::SetCastShadows(false);
			SKSE::log::info("autotest: Ratchet casts no shadows");
		}
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
		s.watchMap = GetPrivateProfileIntW(L"AutoTest", L"bWatchMap", 0, trigger.c_str()) != 0;
		GetPrivateProfileStringW(L"AutoTest", L"fCameraYaw", L"0", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.cameraYaw = std::wcstof(buf, nullptr);
		TestWindow::Install(GetPrivateProfileIntW(L"AutoTest", L"bCorner", 1, trigger.c_str()) != 0);
		// One run per trigger: the next start is a normal game.
		std::filesystem::remove(trigger, ec);
		s.active = true;
		SKSE::log::info("autotest: on (start '{}', flat ground {}, quit {}){}", s.start, s.flatGround, s.quit,
			ec ? " - couldn't delete the trigger file!" : "");
	}

	bool Active() { return s.active; }

	void OnGameLoaded()
	{
		if (!s.active || s.then.empty()) {
			return;
		}
		// From the loaded game to the test place (no new-game questions on the way).
		std::string cmd = std::move(s.then);
		s.then.clear();
		SKSE::log::info("autotest: {}", cmd);
		SKSE::GetTaskInterface()->AddTask([cmd] { RunConsole(cmd); });
	}

	void Install()
	{
		if (!s.active) {
			return;
		}
		// No timed autosaves during a test run (in memory only; the INI files aren't touched).
		// The bSaveOn* switches aren't in either INI collection; `coc` from the main menu saves
		// nothing anyway (checked: no new save after a run).
		SetPref("fAutosaveEveryXMins:SaveGame", false, 1.0e6f, true);
		TestWindow::OnDataLoaded(s.shots, s.watchMap);
		static MainMenuSink sink;
		RE::UI::GetSingleton()->AddEventSink<RE::MenuOpenCloseEvent>(&sink);
	}

	Buttons Press(float a_dt, bool a_grounded, bool a_busy)
	{
		if (!s.active || s.done) {
			return {};
		}
		TestWindow::Keep();
		if (Diagnose(a_dt, a_grounded && !a_busy)) {
			return {};
		}
		s.total += a_dt;
		if (s.total > kTimeout + s.holdEnd) {
			Fail(std::format("the run took over {:.0f} s", kTimeout));
			Finish();
			return {};
		}
		if (s.index >= static_cast<int>(std::size(kCases))) {
			// Stand still a moment, so the last landing's damage (if any) has been dealt.
			s.settle = a_grounded && !a_busy ? s.settle + a_dt : 0.0f;
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
			return {};
		}
		const Case& c = kCases[s.index];

		if (s.pressed < 0.0f) {
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (s.returnPending && s.startPos && player && a_grounded && !a_busy) {
				s.returnPending = false;
				player->SetPosition(*s.startPos, true);
				player->SetHeading(s.startHeading);
				s.settle = 0.0f;
				return {};
			}
			s.settle = a_grounded && !a_busy ? s.settle + a_dt : 0.0f;
			if (!s.startPos && player && s.settle > 1.0f) {
				s.startPos = player->GetPosition();
				s.startHeading = ClearHeading(player->GetPosition(), player->GetAngleZ());
				player->SetHeading(s.startHeading);
				s.settle = 0.0f;
			}
			// The camera behind him (coc starts in first person, where his model isn't drawn).
			if (!s.thirdPerson && s.settle > 0.0f) {
				s.thirdPerson = true;
				ThirdPerson();
				SKSE::log::info("autotest: third person");
			}
			// A fight's target, in place while he settles.
			if (c.targetAt > 0.0f && s.startPos && !s.targetPlaced && s.settle > 0.0f) {
				s.targetPlaced = true;
				PlaceTarget(c.targetAt);
			}
			if (s.settle < kSettle) {
				return {};
			}
			// Standing still (before the second jump: right after a load the screen is still
			// fading in).
			if (s.index == 1) {
				TestWindow::Shot("idle");
			}
			// The run-up: the stick forward (Skyrim's own locomotion runs him) before the press.
			if (s.running < c.run) {
				if (s.running == 0.0f) {
					SKSE::log::info("autotest: {}: running up for {:.1f} s", c.name, c.run);
				}
				Input::SetTestStick(RE::NiPoint2{ 0.0f, 1.0f });
				s.running += a_dt;
				return {};
			}
			if (s.index > 0) {
				CheckHealthAfter(s.index - 1);
			}
			Input::SetTestStick(c.airStick ? std::optional(RE::NiPoint2{ 0.0f, 1.0f }) : std::nullopt);
			s.healthBefore = Health();
			s.pressed = 0.0f;
			s.elapsed = 0.0f;
			if (c.duration > 0.0f) {
				Combat::TakeTally();
				s.targetHealth = TargetHealth();
				if (auto* target = Target(); target && c.fireFrom >= 0.0f) {
					// The shots go for its middle.
					Combat::SetTestAim(target->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, 0.8f * Units() });
				}
			}
			SKSE::log::info("autotest: {} ({}{}{}{}{})", c.name,
				c.hold >= 0.0f ? std::format("jump held {:.2f} s", c.hold) : "no jump",
				c.again > 0.0f ? std::format(", again at {:.2f} s for {:.2f} s", c.again, c.againHold) : "",
				c.dashAt >= 0.0f ? std::format(", dash at {:.2f} s", c.dashAt) : "",
				c.airStick ? ", the stick forward" : "",
				c.duration > 0.0f ? std::format(", a fight against a target {:.1f} m ahead", c.targetAt) : "");
		}

		s.elapsed += a_dt;
		const float t = s.pressed;  // since the case started
		s.pressed += a_dt;
		if (c.duration > 0.0f) {
			// A fight: the wrench presses and the trigger on their times, then the verdict.
			Combat::TestButtons fight;
			for (const float at : c.melee) {
				fight.melee = fight.melee || (at >= 0.0f && t >= at && t < at + a_dt);
			}
			fight.fire = c.fireFrom >= 0.0f && t >= c.fireFrom && t < c.fireTo;
			fight.aim = c.aim && fight.fire;
			fight.throwWrench = c.throwAt >= 0.0f && t >= c.throwAt && t < c.throwAt + a_dt;
			Combat::SetTestButtons(fight);
			if (t >= c.duration && (!a_busy || t >= c.duration + 3.0f)) {
				JudgeFight(c);
				NextCase();
				return {};
			}
			Buttons jump;
			jump.jump = c.hold >= 0.0f && (t == 0.0f || t < c.hold);
			return jump;
		}
		Buttons b;
		b.jump = c.hold >= 0.0f && (t == 0.0f || t < c.hold);
		// The second press: a double jump, and held on, a glide.
		b.jump = b.jump || (c.again > 0.0f && t >= c.again && t < c.again + c.againHold && !s.landed);
		if (c.dashAt >= 0.0f && t >= c.dashAt && !s.dashPressed) {
			s.dashPressed = true;
			b.dash = true;
		}
		if (b.jump || b.dash) {
			return b;
		}
		if (s.landed || (!s.launched && a_grounded && !a_busy)) {
			if (!s.launched) {
				Fail("didn't take off");
			}
			NextCase();
			return {};
		}
		if (s.elapsed > 5.0f) {
			Fail("hasn't landed after 5 s");
			NextCase();
		}
		return {};
	}

	void OnTakeoff()
	{
		if (s.active) {
			s.launched = true;
		}
	}

	void OnMove(std::string_view a_what)
	{
		if (!s.active || s.done || s.index >= static_cast<int>(std::size(kCases))) {
			return;
		}
		std::string what(a_what);
		if (std::ranges::find(s.shotsThisCase, what) != s.shotsThisCase.end()) {
			what += "2";
		}
		s.shotsThisCase.push_back(what);
		std::string name = kCases[s.index].name;
		std::ranges::replace(name, ' ', '_');
		TestWindow::Shot(name + "_" + what);
	}

	void OnLanded(const Jump& a_jump)
	{
		if (!s.active || s.done || s.index >= static_cast<int>(std::size(kCases))) {
			return;
		}
		s.landed = true;
		if (kCases[s.index].duration > 0.0f) {
			// A fight's jump (the Hyper-Strike): the fight's verdict covers it.
			SKSE::log::info("autotest: {}: landed after {:.3f} s, {:.2f} m across", kCases[s.index].name, a_jump.airTime, a_jump.distance);
			return;
		}
		if (!a_jump.replayed) {
			// A wall or ledge took part (or it started out of Skyrim's fall): nothing to compare.
			++s.failed;
			SKSE::log::info("autotest: FAIL {}: the flight met a wall or ledge ({:.2f} m across, landed {:+.3f} m after {:.3f} s)",
				kCases[s.index].name, a_jump.distance, a_jump.landDz, a_jump.airTime);
			return;
		}
		// The sandbox replayed the same frames (time steps, button and stick) on a flat floor, down
		// to where he landed.
		std::string bad;
		if (std::abs(a_jump.apex - a_jump.sandboxApex) > kApexTolerance) {
			bad += " apex";
		}
		if (std::abs(a_jump.apexTime - a_jump.sandboxApexTime) > kApexTimeTolerance + a_jump.apexFrame) {
			bad += " apex-time";
		}
		if (std::abs(a_jump.airTime - a_jump.sandboxAirTime) > kAirTimeTolerance + a_jump.landFrame) {
			bad += " air-time";
		}
		if (s.flatGround && std::abs(a_jump.landDz) > kFloorTolerance) {
			bad += " landing-height";
		}
		if (std::abs(a_jump.distance - a_jump.sandboxDistance) > kDistanceTolerance) {
			bad += " distance";
		}
		if (bad.empty()) {
			++s.passed;
		} else {
			++s.failed;
		}
		SKSE::log::info(
			"autotest: {} {}: apex {:.3f} m (sandbox {:.3f}) at {:.3f} s ({:.3f}) for a {:.3f} s hold, {:.2f} m across ({:.2f}), landed {:+.3f} m after {:.3f} s ({:.3f}){}",
			bad.empty() ? "PASS" : "FAIL", kCases[s.index].name, a_jump.apex, a_jump.sandboxApex, a_jump.apexTime, a_jump.sandboxApexTime,
			a_jump.hold, a_jump.distance, a_jump.sandboxDistance, a_jump.landDz, a_jump.airTime, a_jump.sandboxAirTime,
			bad.empty() ? "" : " - off:" + bad);
	}

	void OnDashed(const Dash& a_dash)
	{
		if (!s.active || s.done || s.index >= static_cast<int>(std::size(kCases)) || kCases[s.index].duration > 0.0f) {
			return;
		}
		s.landed = true;
		// The sandbox replayed the same frames (time steps, buttons and stick) on a flat floor.
		std::string bad;
		if (std::abs(a_dash.distance - a_dash.sandbox.distance) > kDashTolerance) {
			bad += " distance";
		}
		if (std::abs(a_dash.time - a_dash.sandbox.time) > 0.001f) {
			bad += " time";
		}
		if (bad.empty()) {
			++s.passed;
		} else {
			++s.failed;
		}
		SKSE::log::info("autotest: {} {}: {:.2f} m (sandbox {:.2f}) in {:.3f} s ({:.3f}), {:+.2f} m up{}",
			bad.empty() ? "PASS" : "FAIL", kCases[s.index].name, a_dash.distance, a_dash.sandbox.distance, a_dash.time, a_dash.sandbox.time,
			a_dash.climb, bad.empty() ? "" : " - off:" + bad);
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
