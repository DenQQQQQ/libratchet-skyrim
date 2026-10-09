// SPDX-License-Identifier: GPL-3.0-or-later
#include "AutoTest.h"

#include "Camera.h"
#include "Clank.h"
#include "Combat.h"
#include "Fx.h"
#include "Input.h"
#include "Items.h"
#include "Model.h"
#include "Settings.h"
#include "Physics.h"
#include "RatchetDll.h"
#include "Sounds.h"
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
			bool        aim = false;       // and the aim button with it (the whole fight if there's no firing)
			float       minDamage = 0.0f;
			int         minHits = 0;
			float       throwAt = -1.0f;   // the wrench thrown (one frame), s from the start
			bool        awake = false;     // the target's AI on: being hit has to put it in combat
			RE::FormID  target = 0x00023A90;  // the target's base (EncCow)
			bool        crime = false;     // the hit has to cost him a bounty in Whiterun (an assault)
			// A wall across his way, its face this far ahead (metres; 0: none). He has to stick to
			// it and, wallJumpAfter seconds on it with the stick let go, jump off it on a new press
			// and land away from it.
			float       wallAt = 0.0f;
			float       wallJumpAfter = 0.3f;
			// A jump at the target: he mustn't take its body for a wall.
			bool        noWall = false;
			// A ledge: he jumps from a platform ledgeFrom metres up toward a wall ledgeAt metres
			// ahead, its lip ledgeUp above the platform (0: none). He has to catch the lip and,
			// climbAfter seconds on, climb up on a new press.
			float       ledgeAt = 0.0f;
			float       ledgeUp = 2.0f;
			float       ledgeFrom = 4.0f;
			float       climbAfter = 0.8f;  // past the catch's own move, which takes no buttons
			// The Shatterbomb out for this fight (fire held winds a throw up, let go throws); the
			// Burst Pistol again after it.
			bool        shatterbomb = false;
			// In first person (his model isn't drawn there; what he throws still has to be).
			bool        firstPerson = false;
			// A run with no jump, this long (s), the stick forward: no fall in it may last longer
			// than kLongFall (the owner slid down the road's bumps in sim falls of up to 1.6 s).
			float       runFor = 0.0f;
			// The run goes up something placed 1 m ahead: a floor piece tilted this many degrees
			// (rising ahead), or a static (stairs). He has to gain at least minClimb metres.
			float       rampDeg = 0.0f;
			RE::FormID  climb = 0;
			float       minClimb = 0.0f;
			// The aim button held through it (he strafes), and a clip whose name starts with this
			// that has to play in it (the sandbox's choice, from libratchet-anim).
			bool        aimHeld = false;
			const char* wantClip = nullptr;
			// The effects across a cell change: shots at the floor, then this console command (a coc
			// through a load door; "load": the most recent save), then shots again. The new shots'
			// effects have to be alive under the new cell's 3D.
			const char* door = nullptr;
			// He starts on a platform this far up (metres; 0: the ground), its edge half a metre
			// ahead: a long glide off it, for Clank's heli-pack and its sounds.
			float       platformUp = 0.0f;
			// A fight: a clip of the weapon's own that has to have played on it (the wrench's, or
			// with weaponClipOnPistol the pistol's).
			const char* weaponClip = nullptr;
			bool        weaponClipOnPistol = false;
			// The wheel's zoom, notches sent through Skyrim's camera input: out 3 m, in to the
			// closest, once more into first person, and back at the game's distance after.
			bool        zoom = false;
			// The sound lab: one steady file played at distances and volumes from the camera, for
			// a recording (ingame.ps1 -Audio) to measure how Skyrim fades and pans the plugin's
			// sounds. Run only when -Only names it.
			bool        soundLab = false;
			// A fight's shots go this far (metres) past the side of the target's body (its bounds'
			// half width): a near miss the bolt's 0.25 m still hits.
			float       aimBeside = 0.0f;
			// Which way airStick holds the stick (x right, y forward; camera-relative).
			RE::NiPoint2 stick{ 0.0f, 1.0f };
			// A fight: exactly this many Shatterbombs thrown (< 0: not checked).
			int         bombs = -1;
			// The ledge case: this long on the lip (s), crouch is held instead of the climb's press
			// (0: not): he has to let go and come back down.
			float       dropAfter = 0.0f;
			// The ledge case's top is a hair above his jump: he has to end up on it (the sandbox's
			// step up onto a top within 0.35 m), not slide down its face.
			bool        onTop = false;
			// No gun out for this fight (with RatchetMovement.esl: neither equipped): the wrench in his
			// hand throughout, and the aim and fire buttons doing nothing (no strafing, no shots).
			bool        noGun = false;
			// With RatchetMovement.esl: both guns have to be in his inventory, given this game.
			bool        given = false;
		};
		// With the owner's configs (the sandbox, flat floor; heights and distances × scale, times
		// the same at any scale): a press again at 0.4 s double-jumps at 0.42 s (peak 2.8 m at
		// scale 1, 4.34 m at 1.55), and held on he glides from 0.93 s. A ground dash goes 7 m (×
		// scale) in 0.53 s; an air dash flies flat for 0.6 s. The wrench's swings do 20 each
		// (wpn_wrench_melee.json) within 1.8 m (× scale), the Hyper-Strike 50 within 3 m, a Burst
		// Pistol shot 11.36 at 6 shots/s from the hip and 3 aimed. The targets stand where every
		// scale from 1 reaches.
		constexpr Case kCases[] = {
			{ .name = "the guns are given", .hold = -1.0f, .given = true },
			{ "tap", 0.0f },
			{ "short hold", 0.1f },
			{ "medium hold", 0.15f },
			{ "full hold", 0.5f },
			{ "standing jump steered forward", 0.5f, 0.0f, true },
			{ "running jump", 0.5f, 0.8f, true },
			{ .name = "strafe jump", .hold = 0.5f, .aimHeld = true, .wantClip = "strafe_jump" },
			// Aiming, a jump to the side or back is Rift Apart's flip that way (the owner: "strafing
			// flipping" was missing): the sandbox's clip for the jump's way from where he faces.
			{ .name = "strafe flip right", .hold = 0.5f, .run = 0.4f, .airStick = true, .aimHeld = true, .wantClip = "strafe_jump_e", .stick = { 1.0f, 0.0f } },
			{ .name = "strafe flip left", .hold = 0.5f, .run = 0.4f, .airStick = true, .aimHeld = true, .wantClip = "strafe_jump_w", .stick = { -1.0f, 0.0f } },
			{ .name = "strafe flip back", .hold = 0.5f, .run = 0.4f, .airStick = true, .aimHeld = true, .wantClip = "strafe_jump_s", .stick = { 0.0f, -1.0f } },
			{ "double jump", 0.3f, 0.0f, false, 0.4f, 0.1f },
			{ "glide", 0.3f, 0.0f, false, 0.4f, 3.0f },
			// Off a platform 8 m up, the stick forward: seconds of glide (the heli-pack's loop).
			{ .name = "long glide", .hold = 0.3f, .airStick = true, .again = 0.4f, .againHold = 8.0f, .platformUp = 8.0f },
			{ "dash", -1.0f, 0.0f, true, 0.0f, 0.0f, 0.0f },
			{ "air dash", 0.3f, 0.0f, true, 0.0f, 0.0f, 0.35f },
			// A run along the road (the bridge's cobbles), no jump.
			{ .name = "run on the road", .hold = -1.0f, .runFor = 2.0f },
			// Up a 15° and a 30° ramp (a floor piece tilted), and up stone stairs
			// (ImpExtIceStairs01_NoIce, 8.5 m long, 5.2 m up): the owner struggled on small uphills
			// and stairs.
			{ .name = "run up a 15 degree ramp", .hold = -1.0f, .runFor = 2.0f, .rampDeg = 15.0f, .minClimb = 2.5f },
			{ .name = "run up a 30 degree ramp", .hold = -1.0f, .runFor = 2.0f, .rampDeg = 30.0f, .minClimb = 4.0f },
			{ .name = "run up stairs", .hold = -1.0f, .runFor = 2.5f, .climb = 0x000C9B5D, .minClimb = 4.5f },
			// A wall placed 2 m ahead (Whiterun's test spot has none): a full jump at it.
			{ .name = "wall jump", .hold = 0.5f, .airStick = true, .wallAt = 2.4f },
			{ .name = "wrench combo", .hold = -1.0f, .airStick = true, .duration = 2.5f, .targetAt = 1.3f, .melee = { 0.0f, 0.2f, 0.45f }, .minDamage = 55.0f, .minHits = 3,
				.weaponClip = "melee_3" },
			{ .name = "hyper-strike", .hold = 0.3f, .duration = 2.5f, .targetAt = 1.5f, .melee = { 0.35f, -1.0f, -1.0f }, .minDamage = 45.0f, .minHits = 1 },
			{ .name = "burst pistol", .hold = -1.0f, .duration = 2.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 1.0f, .minDamage = 30.0f, .weaponClip = "fire",
				.weaponClipOnPistol = true },
			{ .name = "burst pistol aimed", .hold = -1.0f, .duration = 2.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 1.0f, .aim = true, .minDamage = 20.0f },
			// Aimed 0.15 m past the cow's side: the bolts' 0.25 m (the sandbox's) still hits it.
			{ .name = "a near miss hits", .hold = -1.0f, .duration = 2.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 1.0f, .aim = true, .minDamage = 20.0f,
				.aimBeside = 0.15f },
			{ .name = "wrench throw", .hold = -1.0f, .duration = 2.5f, .targetAt = 8.0f, .minDamage = 25.0f, .minHits = 1, .throwAt = 0.0f },
			// Thrown in a dash (the sandbox doesn't stop a throw for one), at the cow 10 m out (the
			// throw flies flat, as the sandbox's: 14 m out, past the bridge, the road is 1.5 m lower).
			{ .name = "wrench thrown in a dash", .hold = -1.0f, .dashAt = 0.0f, .duration = 3.0f, .targetAt = 10.0f, .minDamage = 25.0f, .minHits = 1, .throwAt = 0.15f },
			// Melee while aiming throws the wrench (Rift Apart's, and the sandbox's).
			{ .name = "melee while aiming throws", .hold = -1.0f, .duration = 2.5f, .targetAt = 8.0f, .melee = { 0.4f, -1.0f, -1.0f }, .aim = true, .minDamage = 25.0f, .minHits = 1 },
			// No gun out: the fire and aim buttons held do nothing, and melee with them swings the
			// wrench (the sandbox without an arsenal), at a hunter (EncHunterNordM, his AI off)
			// ahead. (The cow that close overlapped him as it was placed, and shoved him aside.)
			{ .name = "no gun, the wrench alone", .hold = -1.0f, .airStick = true, .duration = 2.5f, .targetAt = 1.3f, .melee = { 0.3f, -1.0f, -1.0f }, .fireFrom = 0.0f,
				.fireTo = 0.8f, .aim = true, .minDamage = 15.0f, .minHits = 1, .target = 0x00073FBF, .bombs = 0, .noGun = true },
			// A Shatterbomb (170.5 damage within 1 m of its blast, 85.25 at 6 m): wound up 0.4 s,
			// thrown at the cow 8 m ahead.
			{ .name = "shatterbomb", .hold = -1.0f, .duration = 3.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 0.4f, .minDamage = 150.0f, .shatterbomb = true },
			{ .name = "shatterbomb in first person", .hold = -1.0f, .duration = 3.0f, .targetAt = 8.0f, .fireFrom = 0.0f, .fireTo = 0.4f, .minDamage = 150.0f, .shatterbomb = true, .firstPerson = true },
			// The sandbox's busy rules (libarsenal's stow): let go during a dash, nothing is thrown;
			// held through it and let go after, the throw goes (from 7 m on, at the cow 16 m out).
			{ .name = "shatterbomb let go in a dash", .hold = -1.0f, .dashAt = 0.2f, .duration = 2.0f, .targetAt = 16.0f, .fireFrom = 0.0f, .fireTo = 0.45f, .shatterbomb = true,
				.bombs = 0 },
			{ .name = "shatterbomb held through a dash", .hold = -1.0f, .dashAt = 0.2f, .duration = 3.5f, .targetAt = 16.0f, .fireFrom = 0.0f, .fireTo = 1.1f, .minDamage = 75.0f,
				.shatterbomb = true, .bombs = 1 },
			// A neutral hunter (EncHunterImperialM, another than the wrench's), awake, shot from the
			// hip: the shots have to make him fight back too (the owner: the pistol raised nothing).
			{ .name = "a shot makes him fight", .hold = -1.0f, .duration = 2.0f, .targetAt = 6.0f, .fireFrom = 0.0f, .fireTo = 0.5f, .minDamage = 10.0f, .awake = true, .target = 0x00073FBD },
			// Then a neutral hunter (EncHunterNordM), awake, who has to fight back once hit (the
			// owner: hits raised no hostility until a kill).
			{ .name = "a hit makes him fight", .hold = -1.0f, .airStick = true, .duration = 2.0f, .targetAt = 1.3f, .melee = { 0.0f, -1.0f, -1.0f }, .minDamage = 15.0f, .minHits = 1, .awake = true, .target = 0x00073FBF },
			// And a Whiterun guard (GuardWhiterunImperialExterior): hitting him is an assault.
			{ .name = "hitting a guard is a crime", .hold = -1.0f, .airStick = true, .duration = 2.0f, .targetAt = 1.3f, .melee = { 0.6f, -1.0f, -1.0f }, .minDamage = 0.0f, .minHits = 1, .awake = true, .target = 0x001000EC, .crime = true },
			// A full jump at a bear (EncBear, its AI off) 2 m ahead: a big body, but no wall.
			{ .name = "no wall on a bear", .hold = 0.5f, .airStick = true, .targetAt = 2.0f, .target = 0x00023A8A, .noWall = true },
			// A ledge (a placed platform and wall: Rift Apart catches a lip only with 3 m of drop
			// below it and at most 3.5 m above where he last stood). A full jump tops out about a
			// metre short of the wall, inside the catch's reach (1.2 m) and outside the wall
			// stick's (0.775 m).
			{ .name = "ledge grab", .hold = 0.5f, .airStick = true, .ledgeAt = 3.0f },
			// And letting go of it (crouch: Skyrim's Sneak, the sandbox's C).
			{ .name = "ledge drop", .hold = 0.5f, .airStick = true, .ledgeAt = 3.0f, .dropAfter = 1.5f },
			// A top 1.6 m up, 2 m ahead, from the ground: his full jump peaks at 1.5 m, so he only
			// gets onto it by the step up the sandbox's box move gives (0.35 m).
			{ .name = "jump onto a top a hair too high", .hold = 0.5f, .airStick = true, .ledgeAt = 2.0f, .ledgeUp = 1.55f, .ledgeFrom = 0.0f, .onTop = true },
			{ .name = "camera zoom", .hold = -1.0f, .zoom = true },
			{ .name = "sound lab", .hold = -1.0f, .soundLab = true },
			// The effects hang under the player's cell's 3D, which goes at a load door or a load:
			// through the Bannered Mare's door, then a load of the most recent save. Last, as they
			// leave him away from the start.
			{ .name = "effects through a load door", .hold = -1.0f, .door = "coc WhiterunBanneredMare" },
			{ .name = "effects after loading a save", .hold = -1.0f, .door = "load" },
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
		constexpr float kLongFall = 0.3f;            // seconds: a fall in a run on the road longer than this fails it
		constexpr float kTimeout = 240.0f;           // seconds in game for the whole run

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
			std::string only;                   // only the cases whose names contain this (sOnly; empty: all)
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
			RE::ActorHandle witness;  // a crime case's: Skyrim's assault needs one besides the victim
			bool            targetPlaced = false;
			float           targetHealth = 0.0f;  // as the fight began
			std::uint32_t   bounty = 0;           // his bounty in Whiterun as the fight began
			bool            heliSeen = false;     // Clank's heli-pack came out during this case
			std::vector<std::string> clipsSeen;   // his clips during this case
			std::vector<std::string> weaponClipsSeen;  // the case's weapon's own clips
			// The statics placed for this case (removed after it), and what he did on the wall.
			std::vector<RE::ObjectRefHandle> placed;
			bool                        wallPlaced = false;
			float                       onWall = -1.0f;    // seconds on it so far (< 0: not stuck yet)
			float                       wallPress = 0.0f;  // seconds the jump off it has been held
			std::optional<RE::NiPoint3> stuckAt;
			bool                        wallJumped = false;
			bool                        stuckSeen = false;  // on a wall at all during this case
			// The run on the road: its falls, the one going on, and the longest.
			int                         falls = 0;
			float                       fallFor = 0.0f;
			float                       longestFall = 0.0f;
			// What he did on the ledge.
			std::optional<RE::NiPoint3> jumpFrom;          // where he stands on the platform
			float                       onLedge = -1.0f;   // seconds hanging so far (< 0: not yet)
			float                       climbPress = 0.0f; // seconds the climb's jump has been held
			std::optional<RE::NiPoint3> hungAt;
			bool                        climbed = false;
			bool                        dropped = false;   // let go of it (a drop case)
			int                         fightState = -1;   // the sim's state, last logged in a fight
			// The case's gun: 0 not asked for yet, 1 asked for, 2 given up on (it never came out).
			int                         gunStep = 0;
			bool                        doorGunAsked = false;  // asked for again in a door case's new place
			// A no-gun fight: whether he strafed, and whether the wrench was ever out of his hand.
			bool                        strafed = false;
			bool                        wrenchLeftHand = false;
			// A door case: its step (0 shots, 1 through, 2 shots there), the time in it, how long he's
			// stood in the new cell, where he was, the games loaded, and what the effects did there.
			int                 doorStep = 0;
			float               doorTime = 0.0f;
			float               doorSettle = 0.0f;
			RE::TESObjectCELL*  doorFrom = nullptr;
			int                 loads = 0;
			int                 loadsBefore = 0;
			std::size_t         fxLeft = 0;  // effects from the old place still alive in the new one
			std::size_t         fxPeak = 0;
			bool                fxUnder = false;
			bool                ratchetThere = false;  // his model posed on the player there
			bool                clankThere = false;    // and Clank playing on his back
			// The zoom case: its step, the time in it, the wheel's distance at each step and the
			// camera's from him (metres), and whether the last notch went to first person.
			int                 zoomStep = 0;
			float               zoomTime = 0.0f;
			float               zoomFrom = 0.0f;
			float               zoomOut = 0.0f;
			float               zoomIn = 0.0f;
			float               cameraFrom = 0.0f;
			float               cameraOut = 0.0f;
			bool                zoomFirst = false;
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

		void RunConsole(const std::string& a_command);

		// Seconds until the game quits once the run is done (< 0: not quitting).
		float g_quitIn = -1.0f;

		void Finish()
		{
			if (s.done) {
				return;
			}
			s.done = true;
			SKSE::log::info("autotest: done, {} passed, {} failed", s.passed, s.failed);
			if (s.quit) {
				// Quitting with actors pathing crashed the game now and then (a pathfinding job
				// reading Whiterun's navmesh as it went away, right after "done"): AI off first,
				// and a moment for the jobs under way to finish.
				RunConsole("ToggleAI");
				g_quitIn = 1.5f;
			}
		}

		// The previous jump's landing didn't hurt (checked once he's stood still after it: Skyrim
		// applies fall damage after the landing frame).
		void CheckHealthAfter(int a_case)
		{
			if (kCases[a_case].awake || kCases[a_case].door) {
				return;  // its target fought back, or a load set his health
			}
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

		// The -Only filter: names containing any of its |-separated parts (all if it's empty).
		bool Picked(std::string_view a_name)
		{
			if (s.only.empty()) {
				return true;
			}
			for (const auto part : std::views::split(std::string_view(s.only), '|')) {
				const std::string_view want(part.begin(), part.end());
				if (!want.empty() && a_name.find(want) != std::string_view::npos) {
					return true;
				}
			}
			return false;
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
			s.heliSeen = false;
			s.clipsSeen.clear();
			s.weaponClipsSeen.clear();
			for (const auto& placed : s.placed) {
				if (const auto ref = placed.get()) {
					ref->Disable();
					ref->SetDelete(true);
				}
			}
			s.placed.clear();
			if (const auto witness = s.witness.get()) {
				witness->KillImmediate();  // (not disabled: a disabled actor pathing crashed Skyrim's pathing thread)
			}
			s.witness = {};
			s.jumpFrom.reset();
			s.onLedge = -1.0f;
			s.climbPress = 0.0f;
			s.hungAt.reset();
			s.climbed = false;
			s.dropped = false;
			s.fightState = -1;
			s.gunStep = 0;
			s.doorGunAsked = false;
			s.strafed = false;
			s.wrenchLeftHand = false;
			s.wallPlaced = false;
			s.onWall = -1.0f;
			s.wallPress = 0.0f;
			s.stuckAt.reset();
			s.wallJumped = false;
			s.stuckSeen = false;
			s.falls = 0;
			s.fallFor = 0.0f;
			s.longestFall = 0.0f;
			s.doorStep = 0;
			s.doorTime = 0.0f;
			s.doorSettle = 0.0f;
			s.doorFrom = nullptr;
			s.fxLeft = 0;
			s.fxPeak = 0;
			s.fxUnder = false;
			s.ratchetThere = false;
			s.clankThere = false;
			s.zoomStep = 0;
			s.zoomTime = 0.0f;
			s.zoomFirst = false;
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
		void PlaceTarget(float a_metres, RE::FormID a_base)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || !s.startPos) {
				return;
			}
			RE::Actor* target = Target();
			if (target && target->GetActorBase() && target->GetActorBase()->GetFormID() != a_base) {
				target->KillImmediate();  // another kind of target this time (killed: a disabled actor with a path request in flight crashed Skyrim's pathing thread)
				target = nullptr;
			}
			if (!target || target->IsDead()) {
				auto* base = RE::TESForm::LookupByID<RE::TESNPC>(a_base);
				const auto placed = base ? player->PlaceObjectAtMe(base, false) : nullptr;
				target = placed ? placed->As<RE::Actor>() : nullptr;
				if (!target) {
					SKSE::log::info("autotest: couldn't place the target");
					return;
				}
				s.target = target->GetHandle();
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			target->SetPosition(*s.startPos + ahead * (a_metres * Units()) + RE::NiPoint3{ 0.0f, 0.0f, 0.2f * Units() }, true);
			// A crime needs a witness: the victim alone raised no bounty (in the test runs, a bounty
			// came only when a guard on patrol was near). Another guard, 4 m to the side, awake.
			if (kCases[s.index].crime && !s.witness.get()) {
				auto*      base = RE::TESForm::LookupByID<RE::TESNPC>(a_base);
				const auto placed = base ? player->PlaceObjectAtMe(base, false) : nullptr;
				if (auto* witness = placed ? placed->As<RE::Actor>() : nullptr) {
					const RE::NiPoint3 side{ ahead.y, -ahead.x, 0.0f };
					witness->SetPosition(*s.startPos + side * (4.0f * Units()) + ahead * (2.0f * Units()) + RE::NiPoint3{ 0.0f, 0.0f, 0.2f * Units() }, true);
					witness->Update3DPosition(true);
					s.witness = witness->GetHandle();
					SKSE::log::info("autotest: {}: a witness, {}, 4 m to the side", kCases[s.index].name, witness->GetName());
				}
			}
			// Its 3D too: with its AI off it stays where the last fight left it otherwise (pictures
			// showed the cow by him while its body was hit 6 m out).
			target->Update3DPosition(true);
			target->EnableAI(false);
			auto* av = target->AsActorValueOwner();
			av->SetBaseActorValue(RE::ActorValue::kHealth, 1000.0f);
			av->RestoreActorValue(RE::ActorValue::kHealth, 10000.0f);
		}

		// A static placed in the world for a case, turned to the start's heading.
		// a_tilt: the reference's x and y angles (its z is the start's heading).
		RE::ObjectRefHandle Place(RE::FormID a_base, const RE::NiPoint3& a_at, RE::NiPoint2 a_tilt = {})
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* base = RE::TESForm::LookupByID<RE::TESBoundObject>(a_base);
			auto* handler = RE::TESDataHandler::GetSingleton();
			if (!player || !base || !handler) {
				return {};
			}
			return handler->CreateReferenceAtLocation(base, a_at, RE::NiPoint3{ a_tilt.x, a_tilt.y, s.startHeading }, player->GetParentCell(),
				player->GetWorldspace(), nullptr, nullptr, {}, false, false);
		}

		// The climbing cases' slope: a floor piece (WRIntFloorSTMid01Large, 14.6 m square) tilted
		// a_case.rampDeg, its near edge 1 m ahead on the ground; or the a_case.climb static with
		// its near end there. Then the ground ahead as rays find it (a sign of which way it went).
		void PlaceClimb(const Case& a_case)
		{
			constexpr RE::FormID kFloorBase = 0x001044AA;
			constexpr float      kHalf = 512.0f;  // the floor piece's, game units
			if (!s.startPos) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 up{ 0.0f, 0.0f, 1.0f };
			RE::ObjectRefHandle placed;
			if (a_case.rampDeg > 0.0f) {
				const float t = a_case.rampDeg * 3.14159265f / 180.0f;
				const RE::NiPoint3 at = *s.startPos + ahead * (1.0f * Units() + kHalf * std::cos(t)) + up * (kHalf * std::sin(t));
				// A reference's x and y angles tilt its up axis to (-cos x sin y, sin x, cos x cos y)
				// in the world (measured: an x angle alone tilted the floor about the world's x
				// axis, not across the way he faces): rising ahead, it leans back from the way ahead.
				const float n = std::sin(t);
				const float x = std::asin(-n * ahead.y);
				const float y = std::asin(std::clamp(n * ahead.x / std::cos(x), -1.0f, 1.0f));
				placed = Place(kFloorBase, at, { x, y });
			} else if (a_case.climb) {
				// Its near end 1 m ahead (its bounds start 260 units back from its origin).
				placed = Place(a_case.climb, *s.startPos + ahead * (1.0f * Units() + 260.0f));
			}
			s.placed.push_back(placed);
			const auto ref = placed.get();
			SKSE::log::info("autotest: {}: {} ({:08X}, 3D {})", a_case.name, ref ? "placed" : "couldn't place it", ref ? ref->GetFormID() : 0,
				ref && ref->Get3D() ? "loaded" : "not loaded yet");
		}

		// The ground ahead of the start as rays find it now (metres: height), for the climbing
		// cases' log.
		void MeasureAhead(const Case& a_case)
		{
			if (!s.startPos) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 up{ 0.0f, 0.0f, 1.0f };
			std::string heights;
			for (int i = 0; i <= 10; ++i) {
				const RE::NiPoint3 p = *s.startPos + ahead * (static_cast<float>(i) * Units());
				const auto hit = Physics::Ray(p + up * (8.0f * Units()), p - up * (2.0f * Units()));
				heights += hit ? std::format(" {}:{:+.2f}", i, (hit->point.z - s.startPos->z) / Units()) : std::format(" {}:none", i);
			}
			SKSE::log::info("autotest: {}: the ground ahead (m: height){}", a_case.name, heights);
		}

		// The wall: a flat Dragonsreach wall piece (WRIntCastleWallDoubleHighExStr01Quad, 14.6 m
		// wide and 7.3 m tall, its slab 100 units ahead of its origin by its bounds), across his way
		// with its face a_metres ahead of a_from and its top a_top above it.
		constexpr RE::FormID kWallBase = 0x000D6BB6;
		constexpr float      kWallHeight = 512.0f;  // game units
		constexpr float      kWallSlab = 100.0f;

		RE::ObjectRefHandle PlaceWallAt(const RE::NiPoint3& a_from, float a_metres, float a_top)
		{
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 at = a_from + ahead * (a_metres * Units() - kWallSlab) + RE::NiPoint3{ 0.0f, 0.0f, a_top * Units() - kWallHeight };
			return Place(kWallBase, at);
		}

		// The wall case's wall, sunk half a metre into the ground.
		void PlaceWall(float a_metres)
		{
			if (!s.startPos) {
				return;
			}
			const auto wall = PlaceWallAt(*s.startPos, a_metres, kWallHeight / Units() - 0.5f);
			s.placed.push_back(wall);
			SKSE::log::info("autotest: a wall {:.1f} m ahead{}", a_metres, wall ? "" : ": couldn't place it");
		}

		// The ledge case: a floor piece (WRIntFloorSTMid01Large, 14.6 m square, flat at its
		// origin) a_case.ledgeFrom metres up with its front edge half a metre ahead of where he'll
		// stand, the wall a_case.ledgeAt ahead of that spot reaching a_case.ledgeUp above the
		// floor, and another floor piece on the wall as the ledge's top (the wall piece's own top
		// has next to no collision: Dragonsreach's walls meet its ceiling), its front edge on the
		// wall's face; then he's put on the floor.
		void PlaceLedge(const Case& a_case)
		{
			constexpr RE::FormID kFloorBase = 0x001044AA;
			constexpr float      kFloorHalf = 512.0f;  // game units
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || !s.startPos) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 from = *s.startPos + RE::NiPoint3{ 0.0f, 0.0f, a_case.ledgeFrom * Units() };
			const auto platform = Place(kFloorBase, from + ahead * (0.5f * Units() - kFloorHalf));
			const auto wall = PlaceWallAt(from, a_case.ledgeAt, a_case.ledgeUp);
			const auto top = Place(kFloorBase, from + ahead * (a_case.ledgeAt * Units() + kFloorHalf) + RE::NiPoint3{ 0.0f, 0.0f, (a_case.ledgeUp + 0.05f) * Units() });
			s.placed.insert(s.placed.end(), { platform, wall, top });
			s.jumpFrom = from;
			player->SetPosition(from + RE::NiPoint3{ 0.0f, 0.0f, 0.05f * Units() }, true);
			SKSE::log::info("autotest: a platform {:.1f} m up, a ledge {:.1f} m ahead of it with its lip {:.2f} m above it (facing it is {:.2f} in the sim){}",
				a_case.ledgeFrom, a_case.ledgeAt, a_case.ledgeUp + 0.05f, std::atan2(ahead.x, -ahead.y), platform && wall && top ? "" : ": couldn't place it all");
		}

		// The long glide's platform (WRIntFloorSTMid01Large, 14.6 m square, flat at its origin)
		// a_case.platformUp metres up, its front edge half a metre ahead of where he'll stand; then
		// he's put on it.
		void PlacePlatform(const Case& a_case)
		{
			constexpr RE::FormID kFloorBase = 0x001044AA;
			constexpr float      kFloorHalf = 512.0f;  // game units
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || !s.startPos) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 from = *s.startPos + RE::NiPoint3{ 0.0f, 0.0f, a_case.platformUp * Units() };
			const auto platform = Place(kFloorBase, from + ahead * (0.5f * Units() - kFloorHalf));
			s.placed.push_back(platform);
			player->SetPosition(from + RE::NiPoint3{ 0.0f, 0.0f, 0.05f * Units() }, true);
			SKSE::log::info("autotest: a platform {:.1f} m up{}", a_case.platformUp, platform ? "" : ": couldn't place it");
		}

		// The ledge case's wall as Skyrim's collision has it: straight down onto its top at points
		// across its thickness and along it, and out at its face, from where he'll jump (metres).
		void MeasureLedge(const Case& a_case)
		{
			if (!s.jumpFrom) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 right{ ahead.y, -ahead.x, 0.0f };
			const RE::NiPoint3 up{ 0.0f, 0.0f, Units() };
			for (const float side : { -1.0f, 0.0f, 1.0f }) {
				std::string tops;
				for (const float in : { -0.1f, 0.05f, 0.15f, 0.25f, 0.35f, 0.45f, 0.55f }) {
					const RE::NiPoint3 p = *s.jumpFrom + ahead * ((a_case.ledgeAt + in) * Units()) + right * (side * Units());
					const auto hit = Physics::Ray(p + up * 6.0f, p - up * 1.0f);
					tops += hit ? std::format(" {:+.2f}:{:.2f}m({:.2f})", in, (hit->point.z - s.jumpFrom->z) / Units(), hit->normal.z) :
					              std::format(" {:+.2f}:none", in);
				}
				SKSE::log::info("autotest: the ledge's top {:+.0f} m along it (in from its face: height above the platform (normal up)):{}", side, tops);
			}
			std::string faces;
			for (const float h : { 0.5f, 1.0f, 1.5f, 1.8f, 1.9f, 2.0f, 2.1f, 2.3f, 2.6f }) {
				const RE::NiPoint3 p = *s.jumpFrom + up * h;
				const auto hit = Physics::Ray(p, p + ahead * ((a_case.ledgeAt + 1.0f) * Units()));
				faces += hit ? std::format(" {:.1f}:{:.2f}m", h, (hit->point - p).Length() / Units()) : std::format(" {:.1f}:none", h);
			}
			SKSE::log::info("autotest: the ledge's face (height: distance ahead):{}", faces);
		}

		// The ledge case: he caught the lip and climbed up on the new press.
		void JudgeLedge(const Case& a_case, const Jump& a_jump)
		{
			std::string bad;
			if (a_case.onTop) {
				// Onto the top (its lip ledgeUp + 0.05 up), not down its face.
				if (a_jump.landDz < a_case.ledgeUp) {
					bad += " not-on-top";
				}
			} else if (!s.hungAt) {
				bad += " no-catch";
			}
			if (a_case.onTop) {
				// (No catch or climb wanted.)
			} else if (a_case.dropAfter > 0.0f) {
				// Let go: down again, not up on the ledge.
				if (!s.dropped) {
					bad += " no-drop";
				}
				if (s.climbed || a_jump.landDz > a_case.ledgeUp * 0.5f) {
					bad += " not-down";
				}
				// Sneak let him go, and didn't leave him sneaking.
				if (auto* player = RE::PlayerCharacter::GetSingleton(); player && player->IsSneaking()) {
					bad += " sneaking";
				}
			} else if (!s.climbed) {
				bad += " no-climb";
			}
			if (bad.empty()) {
				++s.passed;
			} else {
				++s.failed;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const auto up = [](const RE::NiPoint3& a_p) { return s.jumpFrom ? (a_p.z - s.jumpFrom->z) / Units() : 0.0f; };
			const auto out = [&](const RE::NiPoint3& a_p) { return s.jumpFrom ? a_case.ledgeAt - (a_p - *s.jumpFrom).Dot(ahead) / Units() : 0.0f; };
			SKSE::log::info("autotest: {} {}: {}, {}{}, landed {:+.2f} m from the platform after {:.3f} s (the lip {:.2f} m up){}", bad.empty() ? "PASS" : "FAIL",
				a_case.name,
				s.hungAt ? std::format("caught it {:.2f} m up, {:.2f} m out from the wall", up(*s.hungAt), out(*s.hungAt)) : "never caught it",
				s.climbed ? "climbed" : "never climbed",
				s.stuckAt ? std::format(", stuck to the wall {:.2f} m up, {:.2f} m out", up(*s.stuckAt), out(*s.stuckAt)) : "", a_jump.landDz, a_jump.airTime,
				a_case.ledgeUp + 0.05f, bad.empty() ? "" : " - off:" + bad);
		}

		// The wall case: he stuck to the wall, jumped off it, and landed at least a metre further
		// out from it than where he stuck.
		void JudgeWall(const Case& a_case, const Jump& a_jump)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || !s.startPos) {
				return;
			}
			const RE::NiPoint3 ahead{ std::sin(s.startHeading), std::cos(s.startHeading), 0.0f };
			const RE::NiPoint3 face = *s.startPos + ahead * (a_case.wallAt * Units());
			const auto out = [&](const RE::NiPoint3& a_p) { return (face - a_p).Dot(ahead) / Units(); };  // metres out from its face
			const float landed = out(player->GetPosition());
			std::string bad;
			if (!s.stuckAt) {
				bad += " no-stick";
			}
			if (!s.wallJumped) {
				bad += " no-wall-jump";
			}
			if (s.stuckAt && landed < out(*s.stuckAt) + 1.0f) {
				bad += " not-away";
			}
			if (bad.empty()) {
				++s.passed;
			} else {
				++s.failed;
			}
			SKSE::log::info("autotest: {} {}: {}, {}, landed {:.2f} m out from the wall ({:+.2f} m) after {:.3f} s in the air{}",
				bad.empty() ? "PASS" : "FAIL", a_case.name,
				s.stuckAt ? std::format("stuck {:.2f} m out, {:.2f} m up", out(*s.stuckAt), (s.stuckAt->z - s.startPos->z) / Units()) : "never stuck",
				s.wallJumped ? "jumped off" : "never jumped off", landed, a_jump.landDz, a_jump.airTime, bad.empty() ? "" : " - off:" + bad);
		}

		std::uint32_t WhiterunBounty()
		{
			auto*       player = RE::PlayerCharacter::GetSingleton();
			const auto* whiterun = RE::TESForm::LookupByID<RE::TESFaction>(0x000267EA);  // CrimeFactionWhiterun
			return player && whiterun ? player->GetCrimeGoldValue(whiterun) : 0;
		}

		// A save he's wanted in: Whiterun's guards attack on sight, and a hit on one is no new
		// crime. Forgiven for the run (nothing is saved): the bounty there, and the alarm everyone
		// has raised on him (the guards stayed hostile with the bounty gone).
		void Pardon()
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* whiterun = RE::TESForm::LookupByID<RE::TESFaction>(0x000267EA);
			auto* lists = RE::ProcessLists::GetSingleton();
			if (!player || !whiterun || !lists) {
				return;
			}
			if (const std::uint32_t bounty = player->GetCrimeGoldValue(whiterun); bounty > 0) {
				player->ClearAllCrimeGold(whiterun);
				SKSE::log::info("autotest: the save's bounty in Whiterun ({}) cleared for the run", bounty);
			}
			lists->StopCombatAndAlarmOnActor(player, false);
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

		const char* GunName(Combat::Gun a_gun)
		{
			return a_gun == Combat::Gun::kBurstPistol ? "Burst Pistol" : a_gun == Combat::Gun::kShatterbomb ? "Shatterbomb" : "no gun";
		}

		// The given case: with RatchetMovement.esl loaded, both guns in his inventory, given this game.
		void JudgeGiven(const Case& a_case)
		{
			const int  pistols = Items::Count(Combat::Gun::kBurstPistol);
			const int  bombs = Items::Count(Combat::Gun::kShatterbomb);
			const auto equipped = Items::Equipped();
			const bool ok = Items::Available() && Items::Given() && pistols >= 1 && bombs >= 1;
			++(ok ? s.passed : s.failed);
			SKSE::log::info("autotest: {} {}: RatchetMovement.esl {}, given {}, he carries {} Burst Pistol(s) and {} Shatterbomb(s), equipped: {}", ok ? "PASS" : "FAIL",
				a_case.name, Items::Available() ? "loaded" : "NOT loaded", Items::Given() ? "yes" : "no", pistols, bombs,
				equipped ? GunName(*equipped) : "neither");
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
			if (a_case.bombs >= 0 && tally.bombs != a_case.bombs) {
				bad += " bombs";
			}
			const bool fighting = Target() && Target()->IsInCombat();
			if (a_case.awake && !fighting) {
				bad += " not-in-combat";
			}
			if (a_case.weaponClip) {
				std::string clips;
				for (const auto& clip : s.weaponClipsSeen) {
					clips += (clips.empty() ? "" : ", ") + clip;
				}
				SKSE::log::info("autotest: {}: the {}'s own clips: {}", a_case.name, a_case.weaponClipOnPistol ? "pistol" : "wrench", clips.empty() ? "none" : clips);
				if (std::ranges::find(s.weaponClipsSeen, a_case.weaponClip) == s.weaponClipsSeen.end()) {
					bad += " weapon-clip";
				}
			}
			if (a_case.noGun) {
				// The buttons a gun would take did nothing; the wrench never left his hand.
				SKSE::log::info("autotest: {}: {} shots, {}strafed, the wrench {}", a_case.name, tally.shots, s.strafed ? "" : "never ",
					s.wrenchLeftHand ? "out of his hand at times" : "in his hand throughout");
				if (tally.shots > 0 || tally.throws > 0) {
					bad += " fired";
				}
				if (s.strafed) {
					bad += " strafed";
				}
				if (s.wrenchLeftHand) {
					bad += " no-wrench";
				}
			}
			if (a_case.crime) {
				const std::uint32_t bounty = WhiterunBounty();
				SKSE::log::info("autotest: {}: his bounty in Whiterun {} -> {}", a_case.name, s.bounty, bounty);
				if (bounty <= s.bounty) {
					bad += " no-crime";
				}
			}
			if (a_case.awake) {
				SKSE::log::info("autotest: {}: the target is {}in combat, {}hostile to him", a_case.name, fighting ? "" : "not ",
					Target() && Target()->IsHostileToActor(RE::PlayerCharacter::GetSingleton()) ? "" : "not ");
			}
			if (bad.empty()) {
				++s.passed;
			} else {
				++s.failed;
			}
			SKSE::log::info("autotest: {} {}: the target lost {:.1f} health (at least {:.1f} at difficulty x{:.2f}), {} wrench hits (at least {}), {} shots, {} throws, {} Shatterbombs ({} hit){}",
				bad.empty() ? "PASS" : "FAIL", a_case.name, lost, minDamage, DifficultyMult(), tally.hits, a_case.minHits, tally.shots,
				tally.throws, tally.bombs, tally.blasted, bad.empty() ? "" : " - off:" + bad);
			if (a_case.throwAt >= 0.0f && Combat::ThrownWrench()) {
				SKSE::log::info("autotest: {}: the wrench is still out after {:.1f} s", a_case.name, a_case.duration);
			}
			// An awake target fights on: it goes, so the run can end in peace.
			if (auto* target = Target(); target && a_case.awake) {
				target->KillImmediate();
			}
		}

		// The test place keeps itself clear: wild animals and anyone else hostile to him nearby (a
		// wolf on the Whiterun road bit him through a run) go, except the fight's target.
		void ClearHostiles()
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* lists = RE::ProcessLists::GetSingleton();
			if (!player || !lists) {
				return;
			}
			const float reach = 60.0f * Units();
			const RE::Actor* target = Target();
			const RE::Actor* witness = s.witness.get().get();
			lists->ForEachHighActor([&](RE::Actor* a_actor) {
				if (a_actor && a_actor != player && a_actor != target && a_actor != witness && !a_actor->IsDead() &&
					a_actor->GetPosition().GetDistance(player->GetPosition()) < reach && a_actor->IsHostileToActor(player)) {
					SKSE::log::info("autotest: clearing {} (hostile to him)", a_actor->GetName());
					a_actor->KillImmediate();
				}
				return RE::BSContainer::ForEachResult::kContinue;
			});
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

		// A door case: shots from the hip at the floor 3 m ahead for 0.4 s; then its door (a coc,
		// or a load of the most recent save); once he's stood a second in the new cell, the shots
		// again. Their pieces have to be alive there, under that cell's 3D (the old cell's is gone:
		// a node left hanging on it would draw nothing, or crash the game).
		void Door(const Case& a_case, float a_dt, bool a_grounded, bool a_busy)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player) {
				return;
			}
			if (s.doorStep != 1) {
				const float h = player->GetAngleZ();
				Combat::SetTestAim(player->GetPosition() + RE::NiPoint3{ std::sin(h), std::cos(h), 0.0f } * (3.0f * Units()));
				Combat::SetTestButtons(Combat::TestButtons{ .fire = s.doorTime < 0.4f });
			}
			s.doorTime += a_dt;
			const auto where = [](RE::TESObjectCELL* a_cell) { return a_cell ? std::string(a_cell->GetFormEditorID()) : std::string("?"); };
			switch (s.doorStep) {
			case 0:
				if (s.doorTime >= 0.6f) {
					s.doorFrom = player->GetParentCell();
					s.loadsBefore = s.loads;
					SKSE::log::info("autotest: {}: {} effects alive in {}, under its 3D: {}; now '{}'", a_case.name, Fx::Live(), where(s.doorFrom),
						Fx::UnderPlayersCell(), a_case.door);
					Combat::SetTestButtons(std::nullopt);
					const std::string door = a_case.door;
					SKSE::GetTaskInterface()->AddTask([door] {
						if (door == "load") {
							auto* saves = RE::BGSSaveLoadManager::GetSingleton();
							saves->PopulateSaveList();
							saves->LoadMostRecentSaveGame();
						} else {
							RunConsole(door);
						}
					});
					s.doorStep = 1;
					s.doorTime = 0.0f;
				}
				return;
			case 1:
				{
					// Through: a new cell (a load: the game loaded again) with its 3D, him on his feet.
					auto*      cell = player->GetParentCell();
					auto*      loaded = cell ? cell->GetRuntimeData().loadedData : nullptr;
					const bool through = std::string_view(a_case.door) == "load" ? s.loads > s.loadsBefore : cell != s.doorFrom;
					s.doorSettle = through && loaded && loaded->cell3D && a_grounded && !a_busy ? s.doorSettle + a_dt : 0.0f;
					// The Burst Pistol out there too (a save loaded has its own gun equipped).
					if (s.doorSettle >= 1.0f && Combat::Selected() != Combat::Gun::kBurstPistol && !s.doorGunAsked) {
						s.doorGunAsked = true;
						Combat::Select(Combat::Gun::kBurstPistol);
					}
					if (s.doorSettle >= 1.0f && Combat::Selected() == Combat::Gun::kBurstPistol) {
						ThirdPerson();  // (a coc or a load can leave the camera in first person)
						s.fxLeft = Fx::Live();
						SKSE::log::info("autotest: {}: in {} now, {} effects alive", a_case.name, where(cell), s.fxLeft);
						s.doorStep = 2;
						s.doorTime = 0.0f;
					} else if (s.doorTime > 40.0f) {
						Fail(std::format("never got through (in {} after 40 s)", where(cell)));
						NextCase();
					}
					return;
				}
			default:
				s.fxPeak = std::max(s.fxPeak, Fx::Live());
				s.fxUnder = s.fxUnder || (Fx::Live() > 0 && Fx::UnderPlayersCell());
				if (const auto hand = Model::GunHand(); hand && hand->GetDistance(player->GetPosition()) < 2.5f * Units()) {
					s.ratchetThere = true;
				}
				s.clankThere = s.clankThere || !Clank::Enabled() || !Clank::CurrentClip().empty();
				if (s.doorTime >= 0.8f) {
					const bool ok = s.fxPeak > 0 && s.fxUnder && s.fxLeft == 0 && s.ratchetThere && s.clankThere;
					++(ok ? s.passed : s.failed);
					SKSE::log::info("autotest: {} {}: in {}, {} effects left from the old place, up to {} alive from the shots there, {}; Ratchet {}, Clank {}",
						ok ? "PASS" : "FAIL", a_case.name, where(player->GetParentCell()), s.fxLeft, s.fxPeak, s.fxUnder ? "under its 3D" : "never under its 3D",
						s.ratchetThere ? "posed on him" : "NOT posed on him", s.clankThere ? "playing" : "NOT playing");
					NextCase();
				}
				return;
			}
		}

		// The camera's distance from him (metres), and whether it's first person.
		float CameraFromHim()
		{
			const auto* camera = RE::PlayerCamera::GetSingleton();
			const auto* player = RE::PlayerCharacter::GetSingleton();
			return camera && camera->cameraRoot && player ? camera->cameraRoot->world.translate.GetDistance(player->GetPosition()) / Units() : 0.0f;
		}

		// The zoom case: five notches out (3 m further back), in all the way to the closest (2 m),
		// once more (first person, as Skyrim's zoom goes), and third person again: back at the
		// game's distance. Each step half a second apart, for the camera to get there.
		void ZoomSteps(const Case& a_case, float a_dt)
		{
			s.zoomTime += a_dt;
			if (s.zoomStep > 0 && s.zoomTime < 0.5f) {
				return;
			}
			s.zoomTime = 0.0f;
			auto* camera = RE::PlayerCamera::GetSingleton();
			switch (s.zoomStep++) {
			case 0:
				s.zoomFrom = Camera::ZoomDistance();
				s.cameraFrom = CameraFromHim();
				for (int i = 0; i < 5; ++i) {
					Camera::TestWheel(false);
				}
				return;
			case 1:
				{
					s.zoomOut = Camera::ZoomDistance();
					s.cameraOut = CameraFromHim();
					const int notches = static_cast<int>(std::ceil((s.zoomOut - 2.0f) / 0.6f - 1e-3f));
					for (int i = 0; i < notches; ++i) {
						Camera::TestWheel(true);
					}
					return;
				}
			case 2:
				s.zoomIn = Camera::ZoomDistance();
				SKSE::log::info("autotest: {}: zoomed in to {:.2f} m, the camera {:.2f} m from him", a_case.name, s.zoomIn, CameraFromHim());
				Camera::TestWheel(true);
				return;
			case 3:
				s.zoomFirst = camera && camera->IsInFirstPerson();
				ThirdPerson();
				return;
			default:
				{
					const float back = Camera::ZoomDistance();
					std::string bad;
					if (std::abs(s.zoomOut - (s.zoomFrom + 3.0f)) > 0.01f) {
						bad += " out";
					}
					if (s.cameraOut < s.cameraFrom + 2.0f) {
						bad += " camera-out";
					}
					if (std::abs(s.zoomIn - 2.0f) > 0.01f) {
						bad += " in";
					}
					if (!s.zoomFirst) {
						bad += " no-first-person";
					}
					if (std::abs(back - s.zoomFrom) > 0.01f) {
						bad += " back";
					}
					++(bad.empty() ? s.passed : s.failed);
					SKSE::log::info("autotest: {} {}: the wheel's distance {:.2f} m, 5 notches out {:.2f} m (the camera {:.2f} -> {:.2f} m from him), in to {:.2f} m, then {}, back at {:.2f} m{}",
						bad.empty() ? "PASS" : "FAIL", a_case.name, s.zoomFrom, s.zoomOut, s.cameraFrom, s.cameraOut, s.zoomIn,
						s.zoomFirst ? "first person" : "NOT first person", back, bad.empty() ? "" : " - off:" + bad);
					NextCase();
					return;
				}
			}
		}

		// The sound lab's steps: a steady sound (the thrown wrench's 1.48 s whir, looped) for 1.4 s
		// each, then 0.6 s quiet; where from the camera (metres: right, ahead) or at him, and how
		// loud. Each logs its start ("autotest: sound lab: ..."): audio_check.py --each measures them.
		struct LabStep
		{
			const char* label;
			float       right, ahead;
			bool        atHim;
			float       volume;
		};
		constexpr LabStep kLab[] = {
			{ "near the camera", 0.0f, 0.3f, false, 1.0f },
			{ "at him", 0.0f, 0.0f, true, 1.0f },
			{ "10 m ahead", 0.0f, 10.0f, false, 1.0f },
			{ "20 m ahead", 0.0f, 20.0f, false, 1.0f },
			{ "40 m ahead", 0.0f, 40.0f, false, 1.0f },
			{ "80 m ahead", 0.0f, 80.0f, false, 1.0f },
			{ "near the camera at half volume", 0.0f, 0.3f, false, 0.5f },
			{ "10 m right", 10.0f, 0.0f, false, 1.0f },
			{ "10 m left", -10.0f, 0.0f, false, 1.0f },
		};
		constexpr const char* kLabFile = "Sound\\FX\\RatchetMovement\\wrench_throw_loop\\900426063_loop.wav";
		int   g_labStep = 0;
		float g_labTime = 0.0f;

		void LabSteps(float a_dt)
		{
			const bool playing = g_labStep % 2 == 1;  // odd: a step plays; even: quiet before the next
			g_labTime += a_dt;
			if (g_labTime < (playing ? 1.4f : 0.6f)) {
				return;
			}
			g_labTime = 0.0f;
			Sounds::StopTest();
			const int next = (g_labStep + 1) / 2;  // the step to play now (after a quiet one)
			++g_labStep;
			if (playing) {
				SKSE::log::info("autotest: sound lab: quiet");
				return;
			}
			if (next >= static_cast<int>(std::size(kLab))) {
				++s.passed;
				SKSE::log::info("autotest: PASS sound lab: {} steps played (measure them in the recording)", std::size(kLab));
				g_labStep = 0;
				NextCase();
				return;
			}
			const auto& step = kLab[next];
			const auto* camera = RE::PlayerCamera::GetSingleton();
			const auto* player = RE::PlayerCharacter::GetSingleton();
			if (!camera || !camera->cameraRoot || !player) {
				return;
			}
			const auto&  view = camera->cameraRoot->world;
			RE::NiPoint3 at = step.atHim ? player->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, Units() } :
			                               view.translate + view.rotate.GetVectorX() * (step.right * Units()) + view.rotate.GetVectorY() * (step.ahead * Units());
			SKSE::log::info("autotest: sound lab: {} ({:.1f} m from the camera, volume {:.2f})", step.label, at.GetDistance(view.translate) / Units(), step.volume);
			Sounds::PlayTest(kLabFile, at, step.volume);
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
		// Every frame traced in a test run, whatever the INI says.
		if (GetPrivateProfileIntW(L"AutoTest", L"bTraceJumps", 0, trigger.c_str()) != 0) {
			Settings::Get().traceJumps = true;
		}
		GetPrivateProfileStringW(L"AutoTest", L"sOnly", L"", buf, static_cast<DWORD>(std::size(buf)), trigger.c_str());
		s.only.clear();
		for (const wchar_t* c = buf; *c; ++c) {
			s.only.push_back(static_cast<char>(*c));  // case names are ASCII
		}
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
		++s.loads;
		if (!s.active) {
			return;
		}
		Pardon();
		if (s.then.empty()) {
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
		// Nor Skyrim's auto-vanity camera, which starts circling him after 120 s with no input (the
		// tests' buttons aren't input to Skyrim): the full run's later cases were steered by it.
		SetPref("fAutoVanityModeDelay:Camera", false, 1.0e6f, true);
		TestWindow::OnDataLoaded(s.shots, s.watchMap);
		static MainMenuSink sink;
		RE::UI::GetSingleton()->AddEventSink<RE::MenuOpenCloseEvent>(&sink);
	}

	Buttons Press(float a_dt, bool a_grounded, bool a_busy, std::uint32_t a_state)
	{
		if (g_quitIn > 0.0f) {
			g_quitIn -= a_dt;
			if (g_quitIn <= 0.0f) {
				RE::Main::GetSingleton()->quitGame = true;
			}
		}
		if (!s.active || s.done) {
			return {};
		}
		TestWindow::Keep();
		if (Diagnose(a_dt, a_grounded && !a_busy)) {
			return {};
		}
		s.total += a_dt;
		// Not while a crime is being reported: the guards it turns hostile are its witnesses (cleared
		// at once, the bounty came or not by where their patrol had them).
		if (s.index >= static_cast<int>(std::size(kCases)) || !kCases[s.index].crime) {
			ClearHostiles();
		}
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
		if (!Picked(c.name) || (c.soundLab && s.only.empty())) {
			NextCase();
			return {};
		}

		if (s.pressed < 0.0f) {
			auto* player = RE::PlayerCharacter::GetSingleton();
			// (Not for a door case: he may be in another cell by then.)
			if (s.returnPending && s.startPos && player && a_grounded && !a_busy && !c.door) {
				s.returnPending = false;
				player->SetPosition(*s.startPos, true);
				// Turned so the stick's way (camera-relative: his own, the camera behind him) is the
				// start's heading, the way with room.
				player->SetHeading(s.startHeading - (c.airStick ? std::atan2(c.stick.x, c.stick.y) : 0.0f));
				s.settle = 0.0f;
				return {};
			}
			// Settled before the run-up; once it's begun he's running (and the sim moves him).
			s.settle = s.running > 0.0f ? kSettle : (a_grounded && !a_busy ? s.settle + a_dt : 0.0f);
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
				if (c.crime) {
					Pardon();  // (he may be wanted again: the run's other fights, or the save)
				}
				PlaceTarget(c.targetAt, c.target);
			}
			if (c.wallAt > 0.0f && s.startPos && !s.wallPlaced && s.settle > 0.0f) {
				s.wallPlaced = true;
				PlaceWall(c.wallAt);
			}
			if (c.ledgeAt > 0.0f && s.startPos && !s.wallPlaced && s.settle > 0.0f) {
				s.wallPlaced = true;
				PlaceLedge(c);
				return {};
			}
			if ((c.rampDeg > 0.0f || c.climb) && s.startPos && !s.wallPlaced && s.settle > 0.0f) {
				s.wallPlaced = true;
				PlaceClimb(c);
			}
			if (c.platformUp > 0.0f && s.startPos && !s.wallPlaced && s.settle > 0.0f) {
				s.wallPlaced = true;
				PlacePlatform(c);
				return {};
			}
			// The case's gun (the Burst Pistol unless it says), out before it starts: with
			// RatchetMovement.esl it's equipped, which Skyrim does a frame or so later.
			const Combat::Gun gun = c.noGun ? Combat::Gun::kNone : c.shatterbomb ? Combat::Gun::kShatterbomb : Combat::Gun::kBurstPistol;
			if (s.gunStep == 0 && s.settle > 0.0f) {
				s.gunStep = 1;
				if (Combat::Selected() != gun) {
					SKSE::log::info("autotest: {}: the {} out (from the {})", c.name, GunName(gun), GunName(Combat::Selected()));
				}
				Combat::Select(gun);  // (asked for even if it seems out: the first frames on foot still follow what's equipped)
			}
			if (s.settle < kSettle) {
				return {};
			}
			if (s.gunStep == 1 && Combat::Selected() != gun) {
				if (s.settle < kSettle + 2.0f) {
					return {};
				}
				s.gunStep = 2;
				Fail(std::format("the {} wasn't out 2 s after it was asked for (the {} is)", GunName(gun), GunName(Combat::Selected())));
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
				// How fast he gets going from standing (the owner: "slow acceleration"; Skyrim's own
				// locomotion idles 0.14 s and reaches its run after 0.4 s).
				if (const auto* ctrl = player ? player->GetCharController() : nullptr) {
					RE::hkVector4 v;
					ctrl->GetLinearVelocityImpl(v);
					const float speed = std::sqrt(v.quad.m128_f32[0] * v.quad.m128_f32[0] + v.quad.m128_f32[1] * v.quad.m128_f32[1]);
					SKSE::log::info("autotest: run-up {:.3f} s: {:.2f} m/s", s.running, speed);
				}
				// (A strafe flip's run-up: aiming, the stick its way.)
				Input::SetTestStick(c.stick);
				if (c.aimHeld) {
					Combat::SetTestButtons(Combat::TestButtons{ .aim = true });
				}
				s.running += a_dt;
				return {};
			}
			if (s.index > 0) {
				CheckHealthAfter(s.index - 1);
			}
			Input::SetTestStick(c.airStick ? std::optional(c.stick) : std::nullopt);
			s.healthBefore = Health();
			s.pressed = 0.0f;
			s.elapsed = 0.0f;
			if (c.ledgeAt > 0.0f) {
				MeasureLedge(c);
			}
			if (c.rampDeg > 0.0f || c.climb) {
				MeasureAhead(c);
			}
			if (c.duration > 0.0f) {
				if (c.firstPerson) {
					if (auto* camera = RE::PlayerCamera::GetSingleton()) {
						camera->ForceFirstPerson();
					}
					s.thirdPerson = false;  // the next case puts the camera back behind him
				}
				Combat::TakeTally();
				s.targetHealth = TargetHealth();
				s.bounty = WhiterunBounty();
				if (auto* target = Target(); target && c.crime) {
					auto* him = RE::PlayerCharacter::GetSingleton();
					SKSE::log::info("autotest: {}: before the hit, {} is {}in combat, {}hostile to him; he's {}in combat", c.name, target->GetName(),
						target->IsInCombat() ? "" : "not ", target->IsHostileToActor(him) ? "" : "not ", him->IsInCombat() ? "" : "not ");
				}
				if (auto* target = Target(); target && c.awake) {
					target->EnableAI(true);  // awake from the fight's start (it would wander off while he settles)
				}
				if (auto* target = Target(); target && (c.fireFrom >= 0.0f || c.throwAt >= 0.0f)) {
					// The shots (and the throw) go for its middle, or past its side.
					RE::NiPoint3 aim = target->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, 0.8f * Units() };
					if (c.aimBeside > 0.0f) {
						const float        half = (target->GetBoundMax().x - target->GetBoundMin().x) * 0.5f * target->GetScale();
						const RE::NiPoint3 right{ std::cos(s.startHeading), -std::sin(s.startHeading), 0.0f };
						aim += right * (half + c.aimBeside * Units());
						SKSE::log::info("autotest: {}: aimed {:.2f} m from its middle ({:.2f} m past its side)", c.name, (half + c.aimBeside * Units()) / Units(), c.aimBeside);
					}
					Combat::SetTestAim(aim);
				}
				if (auto* target = Target(); target && player && s.startPos) {
					SKSE::log::info("autotest: {}: he's {:.2f} m from the start, the target {:.2f} m from him ({:.2f} m from the start)", c.name,
						player->GetPosition().GetDistance(*s.startPos) / Units(), target->GetPosition().GetDistance(player->GetPosition()) / Units(),
						target->GetPosition().GetDistance(*s.startPos) / Units());
					// Where the camera and his drawn hand are from him (a picture showed him at the target).
					const auto* camera = RE::PlayerCamera::GetSingleton();
					const auto  hand = Model::GunHand();
					SKSE::log::info("autotest: {}: the camera {:.2f} m from him, his hand drawn {:.2f} m from him, the target's 3D {:.2f} m from him", c.name,
						camera && camera->cameraRoot ? camera->cameraRoot->world.translate.GetDistance(player->GetPosition()) / Units() : -1.0f,
						hand ? hand->GetDistance(player->GetPosition()) / Units() : -1.0f,
						target->Get3D() ? target->Get3D()->world.translate.GetDistance(player->GetPosition()) / Units() : -1.0f);
				}
			}
			SKSE::log::info("autotest: {} ({}{}{}{}{}{})", c.name,
				c.hold >= 0.0f ? std::format("jump held {:.2f} s", c.hold) : "no jump",
				c.again > 0.0f ? std::format(", again at {:.2f} s for {:.2f} s", c.again, c.againHold) : "",
				c.dashAt >= 0.0f ? std::format(", dash at {:.2f} s", c.dashAt) : "",
				!c.airStick       ? "" :
				c.stick.x > 0.5f  ? ", the stick right" :
				c.stick.x < -0.5f ? ", the stick left" :
				c.stick.y < -0.5f ? ", the stick back" :
				                    ", the stick forward",
				c.duration > 0.0f ? std::format(", a fight against a target {:.1f} m ahead", c.targetAt) : "",
				c.wallAt > 0.0f ? std::format(", a wall {:.1f} m ahead, jumped off {:.2f} s after he sticks", c.wallAt, c.wallJumpAfter) : "");
		}

		s.elapsed += a_dt;
		const float t = s.pressed;  // since the case started
		s.pressed += a_dt;
		s.heliSeen = s.heliSeen || Clank::HeliOut();
		if (const std::string clip(Model::CurrentClip()); !clip.empty() && std::ranges::find(s.clipsSeen, clip) == s.clipsSeen.end()) {
			s.clipsSeen.push_back(clip);
		}
		if (c.weaponClip) {
			if (const std::string clip(Model::WeaponClip(c.weaponClipOnPistol)); !clip.empty() && std::ranges::find(s.weaponClipsSeen, clip) == s.weaponClipsSeen.end()) {
				s.weaponClipsSeen.push_back(clip);
			}
		}
		if (a_state == RATCHET_STATE_WALL_STICK && !s.stuckSeen) {
			s.stuckSeen = true;
			if (auto* player = RE::PlayerCharacter::GetSingleton(); player && c.ledgeAt > 0.0f) {
				s.stuckAt = player->GetPosition();
			}
		}
		// Once the heli-pack is all the way out (the glide's own clip), a picture of it.
		if (Clank::CurrentClip() == "glide" && std::ranges::find(s.shotsThisCase, "heli") == s.shotsThisCase.end()) {
			OnMove("heli");
		}
		if (c.given) {
			JudgeGiven(c);
			NextCase();
			return {};
		}
		if (c.door) {
			Door(c, a_dt, a_grounded, a_busy);
			return {};
		}
		if (c.zoom) {
			ZoomSteps(c, a_dt);
			return {};
		}
		if (c.soundLab) {
			LabSteps(a_dt);
			return {};
		}
		if (c.duration > 0.0f) {
			// (The sim's states through a fight, logged as they change: a flaky combo said why.)
			if (static_cast<int>(a_state) != s.fightState) {
				SKSE::log::info("autotest: {} {:.3f} s: state {}", c.name, t, a_state);
				s.fightState = static_cast<int>(a_state);
			}
			if (c.noGun) {
				s.strafed = s.strafed || Combat::Strafing();
				s.wrenchLeftHand = s.wrenchLeftHand || Combat::InHand() != Combat::Hand::kWrench;
			}
			// A fight: the wrench presses and the trigger on their times, then the verdict. A press
			// is the one frame whose span [t, t + dt) holds its time (frames tile the clock, so it's
			// exactly one: "the first frame starting within a frame after it" missed one now and then
			// when a long frame came before a shorter one, and the combo lost its third swing).
			const auto inFrame = [&](float a_at) { return a_at >= 0.0f && a_at >= t && a_at < t + a_dt; };
			Combat::TestButtons fight;
			for (const float at : c.melee) {
				fight.melee = fight.melee || inFrame(at);
			}
			fight.fire = c.fireFrom >= 0.0f && t >= c.fireFrom && t < c.fireTo;
			fight.aim = c.aim && (fight.fire || c.fireFrom < 0.0f);
			fight.throwWrench = inFrame(c.throwAt);
			Combat::SetTestButtons(fight);
			if (t >= c.duration && (!a_busy || t >= c.duration + 3.0f)) {
				JudgeFight(c);
				NextCase();
				return {};
			}
			Buttons jump;
			jump.jump = c.hold >= 0.0f && (t == 0.0f || t < c.hold);
			// A dash in the fight (the Shatterbomb's busy rules), pressed one frame.
			if (c.dashAt >= 0.0f && t >= c.dashAt && !s.dashPressed) {
				s.dashPressed = true;
				jump.dash = true;
			}
			return jump;
		}
		if (c.runFor > 0.0f) {
			// The run: the stick forward, its falls counted, then the verdict.
			Input::SetTestStick(RE::NiPoint2{ 0.0f, 1.0f });
			if (a_state == RATCHET_STATE_FALL) {
				if (s.fallFor == 0.0f) {
					++s.falls;
				}
				s.fallFor += a_dt;
				s.longestFall = std::max(s.longestFall, s.fallFor);
			} else {
				s.fallFor = 0.0f;
			}
			auto*       player = RE::PlayerCharacter::GetSingleton();
			const float climbed = player && s.startPos ? (player->GetPosition().z - s.startPos->z) / Units() : 0.0f;
			// A climb ends once he's that high (on he'd run off its far end).
			const bool reached = c.minClimb > 0.0f && climbed >= c.minClimb;
			if (t >= c.runFor || reached) {
				std::string bad;
				if (s.longestFall > kLongFall) {
					bad += " long-fall";
				}
				if (c.minClimb > 0.0f && !reached) {
					bad += " climb";
				}
				if (bad.empty()) {
					++s.passed;
				} else {
					++s.failed;
				}
				SKSE::log::info("autotest: {} {}: {:.1f} m in {:.2f} s, {:+.2f} m up (at least {:.2f}), {} falls, the longest {:.3f} s (at most {:.2f}){}",
					bad.empty() ? "PASS" : "FAIL", c.name, player && s.startPos ? player->GetPosition().GetDistance(*s.startPos) / Units() : 0.0f,
					t, climbed, c.minClimb, s.falls, s.longestFall, kLongFall, bad.empty() ? "" : " - off:" + bad);
				NextCase();
			}
			return {};
		}
		if (c.aimHeld) {
			Combat::SetTestButtons(Combat::TestButtons{ .aim = true });
		}
		Buttons b;
		b.jump = c.hold >= 0.0f && (t == 0.0f || t < c.hold);
		// The second press: a double jump, and held on, a glide.
		b.jump = b.jump || (c.again > 0.0f && t >= c.again && t < c.again + c.againHold && !s.landed);
		if (c.dashAt >= 0.0f && t >= c.dashAt && !s.dashPressed) {
			s.dashPressed = true;
			b.dash = true;
		}
		// The wall: once he's on it the stick is let go, and wallJumpAfter seconds on, the jump is
		// pressed again (held 0.15 s).
		if (c.wallAt > 0.0f) {
			if (a_state == RATCHET_STATE_WALL_STICK) {
				if (s.onWall < 0.0f) {
					s.onWall = 0.0f;
					if (auto* player = RE::PlayerCharacter::GetSingleton()) {
						s.stuckAt = player->GetPosition();
					}
					Input::SetTestStick(std::nullopt);
				}
				s.onWall += a_dt;
			}
			s.wallJumped = s.wallJumped || a_state == RATCHET_STATE_WALL_JUMP;
			if (s.onWall >= c.wallJumpAfter && s.wallPress < 0.15f) {
				s.wallPress += a_dt;
				b.jump = true;
			}
		}
		// The ledge: the same, hanging from it, the press climbs.
		if (c.ledgeAt > 0.0f) {
			if (a_state == RATCHET_STATE_LEDGE_HANG) {
				if (s.onLedge < 0.0f) {
					s.onLedge = 0.0f;
					if (auto* player = RE::PlayerCharacter::GetSingleton()) {
						s.hungAt = player->GetPosition();
					}
					Input::SetTestStick(std::nullopt);
				}
				s.onLedge += a_dt;
			}
			s.climbed = s.climbed || a_state == RATCHET_STATE_LEDGE_CLIMB;
			if (c.dropAfter > 0.0f) {
				// Letting go: crouch held a moment, and then he's falling (or dropping with a turn).
				if (s.onLedge >= c.dropAfter && s.climbPress < 0.2f) {
					s.climbPress += a_dt;
					b.crouch = true;
				}
				// (Let go by this crouch, or by Skyrim's own Sneak pressed sooner.)
				s.dropped = s.dropped || (s.onLedge >= 0.0f && (a_state == RATCHET_STATE_FALL || a_state == RATCHET_STATE_LEDGE_DROP));
			} else if (s.onLedge >= c.climbAfter && s.climbPress < 0.15f) {
				s.climbPress += a_dt;
				b.jump = true;
			}
		}
		if (b.jump || b.dash || b.crouch) {
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
		// (A file name: no spaces, and nothing Windows keeps out of one: a colon would write an NTFS stream.)
		std::string name = kCases[s.index].name;
		std::ranges::replace_if(name, [](char a_c) { return a_c == ' ' || std::string_view("<>:\"/\\|?*").contains(a_c); }, '_');
		TestWindow::Shot(name + "_" + what);
	}

	void OnLanded(const Jump& a_jump)
	{
		if (!s.active || s.done || s.index >= static_cast<int>(std::size(kCases))) {
			return;
		}
		if (s.pressed < 0.0f) {
			// Before the case began: off the last case's ramp or stairs as they went away.
			SKSE::log::info("autotest: {}: a landing before it began ({:+.2f} m after {:.3f} s), not counted", kCases[s.index].name, a_jump.landDz,
				a_jump.airTime);
			return;
		}
		s.landed = true;
		if (kCases[s.index].duration > 0.0f) {
			// A fight's jump (the Hyper-Strike): the fight's verdict covers it.
			SKSE::log::info("autotest: {}: landed after {:.3f} s, {:.2f} m across", kCases[s.index].name, a_jump.airTime, a_jump.distance);
			return;
		}
		if (kCases[s.index].wallAt > 0.0f) {
			JudgeWall(kCases[s.index], a_jump);
			return;
		}
		if (kCases[s.index].ledgeAt > 0.0f) {
			JudgeLedge(kCases[s.index], a_jump);
			return;
		}
		if (kCases[s.index].noWall) {
			if (s.stuckSeen) {
				++s.failed;
			} else {
				++s.passed;
			}
			SKSE::log::info("autotest: {} {}: {}, landed {:.2f} m across after {:.3f} s", s.stuckSeen ? "FAIL" : "PASS", kCases[s.index].name,
				s.stuckSeen ? "he stuck to its body as to a wall" : "no wall stick", a_jump.distance, a_jump.airTime);
			return;
		}
		if (!a_jump.replayed) {
			// A wall or ledge took part (or it started out of Skyrim's fall): nothing to compare.
			++s.failed;
			SKSE::log::info("autotest: FAIL {}: the flight met a wall or ledge ({:.2f} m across, landed {:+.3f} m after {:.3f} s)",
				kCases[s.index].name, a_jump.distance, a_jump.landDz, a_jump.airTime);
			return;
		}
		if (kCases[s.index].aimHeld) {
			// Aiming, the sim jumps Rift Apart's strafe jump (lower); the flat replay doesn't aim,
			// so its numbers aren't this jump's.
			SKSE::log::info("autotest: {}: apex {:.3f} m at {:.3f} s, landed {:+.3f} m after {:.3f} s (aimed: not replayed)", kCases[s.index].name,
				a_jump.apex, a_jump.apexTime, a_jump.landDz, a_jump.airTime);
		} else {
			// The sandbox replayed the same frames (time steps, button and stick) on a flat floor,
			// down to where he landed.
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
		// A clip the sandbox picks for it has to have played.
		if (const char* want = kCases[s.index].wantClip) {
			const bool seen = std::ranges::any_of(s.clipsSeen, [&](const std::string& a_clip) { return a_clip.starts_with(want); });
			if (seen) {
				++s.passed;
			} else {
				++s.failed;
			}
			std::string clips;
			for (const auto& clip : s.clipsSeen) {
				clips += (clips.empty() ? "" : ", ") + clip;
			}
			SKSE::log::info("autotest: {} {}: {} {} (clips: {})", seen ? "PASS" : "FAIL", kCases[s.index].name, want, seen ? "played" : "never played", clips);
		}
		// A glide (the second press held on): Clank's heli-pack has to have come out.
		if (kCases[s.index].againHold > 1.0f && Clank::Enabled()) {
			if (s.heliSeen) {
				++s.passed;
			} else {
				++s.failed;
			}
			SKSE::log::info("autotest: {} {}: Clank's heli-pack {}", s.heliSeen ? "PASS" : "FAIL", kCases[s.index].name,
				s.heliSeen ? "came out" : "never came out");
		}
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

	bool TraceRays()
	{
		return s.active && !s.done && s.index < static_cast<int>(std::size(kCases)) && kCases[s.index].ledgeAt > 0.0f && s.pressed >= 0.0f;
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
