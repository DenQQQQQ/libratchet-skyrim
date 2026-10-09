// SPDX-License-Identifier: GPL-3.0-or-later
#include "LoadScreens.h"

#include "Settings.h"

namespace LoadScreens
{
	namespace
	{
		constexpr std::string_view kFile = "RatchetMovement.esl";

		// How each model stands on a loading screen: RatchetMovement.esl's static, its size (the
		// models are in game units, facing -y: Ratchet is 84 tall), its turn (degrees about x, y,
		// z) and where it's moved to (game units). Ours, fitted to the game's own: a standing
		// Imperial is shown at 1.4, turned 195 (her +y face to -y, and 15 more), lowered 140; a
		// greatsword at 1.0, turned 45, lowered 36.
		struct Look
		{
			RE::FormID   stat;
			float        scale;
			std::int16_t turn[3];
			float        offset[3];
		};
		// (The screen shows about 135 units' height round the camera's aim, the model a third of
		// the way in from the left. Measured on the gallery's pictures: Ratchet at 1.25, 105 units
		// tall, fills 78% of it.)
		constexpr Look kLooks[] = {
			{ 0x810, 1.25f, { 0, 0, 15 }, { -10.0f, 0.0f, -80.0f } },  // Ratchet, standing in his idle
			{ 0x811, 2.0f, { 0, 0, 195 }, { -5.0f, 0.0f, -74.0f } },   // Clank, the same (his model faces back: he rides backwards)
			{ 0x812, 1.9f, { 0, 0, 45 }, { -15.0f, 0.0f, -55.0f } },   // the OmniWrench
			{ 0x813, 1.6f, { 0, 0, -70 }, { -10.0f, 0.0f, -37.0f } },  // the Burst Pistol
			{ 0x814, 4.0f, { 0, 0, 30 }, { 0.0f, 0.0f, -25.0f } },     // the Shatterbomb
			{ 0x815, 4.5f, { 20, 0, 30 }, { 0.0f, 0.0f, -45.0f } },    // a gold bolt
		};

		// The tips: his moves on Skyrim's controls (ours, for this mod; the defaults' keys).
		constexpr const char* kTips[] = {
			"Press Jump again in the air to double jump. Hold it to glide on Clank's Heli-Pack.",
			"Sprint dashes, on the ground and in the air.",
			"Ready Weapon swings the OmniWrench: keep pressing for the full combo. In the air, it's a Hyper-Strike.",
			"Shout throws the OmniWrench, and it comes back to you. So does melee while you aim.",
			"Equip the Burst Pistol or the Shatterbomb as a weapon to fight with it. With neither, the OmniWrench is all you need.",
			"Hold fire with the Shatterbomb to wind up the throw, and let go to lob it.",
			"Whatever you defeat bursts into bolts. Come within reach and they fly to you.",
			"Your thrown OmniWrench pulls in the bolts it passes near.",
			"Jump at a wall to stick to it, then jump again to leap off.",
			"Jump at a ledge's lip to catch it, and press Jump to climb up. Sneak lets go.",
			"Aim, and the camera moves over his shoulder: jumps become flips.",
		};

		std::string ModelPath(const RE::TESBoundObject* a_object)
		{
			const auto* model = a_object ? a_object->As<RE::TESModel>() : nullptr;
			return model && model->GetModel() ? model->GetModel() : "";
		}

		// The models found, with their looks.
		std::vector<std::pair<RE::TESBoundObject*, const Look*>> g_looks;
		// The camera move most of the game's screens have (they're framed by it).
		std::string g_cameraPath;

		void Apply(RE::TESLoadScreen::LoadNIFData* a_nif, RE::TESBoundObject* a_model, const Look& a_look)
		{
			a_nif->loadNIF = a_model;
			a_nif->initialScale = a_look.scale;
			for (int k = 0; k < 3; ++k) {
				a_nif->rotationConstraints[k] = a_look.turn[k];
				a_nif->initialTranslationOffset[k] = a_look.offset[k];
			}
			// The same camera move on every screen (its own was framed for its model: on some, the
			// same model sat lower), most of the game's own screens' one.
			if (!g_cameraPath.empty()) {
				a_nif->cameraPath.SetModel(g_cameraPath.c_str());
			}
		}
	}

	std::string ShowOnly(int a_look)
	{
		auto* data = RE::TESDataHandler::GetSingleton();
		if (!data || g_looks.empty()) {
			return {};
		}
		const auto& [model, look] = g_looks[static_cast<std::size_t>(a_look) % g_looks.size()];
		for (auto* screen : data->GetFormArray<RE::TESLoadScreen>()) {
			if (screen && screen->loadNIFData) {
				Apply(screen->loadNIFData, model, *look);
			}
		}
		return ModelPath(model);
	}

	std::pair<std::string, std::string> Describe(const RE::TESLoadScreen* a_screen)
	{
		if (!a_screen) {
			return {};
		}
		const auto* nif = a_screen->loadNIFData;
		return { nif ? ModelPath(nif->loadNIF) : std::string(), a_screen->loadingText.c_str() ? a_screen->loadingText.c_str() : "" };
	}

	void Install()
	{
		auto* data = RE::TESDataHandler::GetSingleton();
		if (!data || !data->LookupModByName(kFile)) {
			SKSE::log::info("loading screens: no {} loaded: Skyrim's own", kFile);
			return;
		}
		auto& looks = g_looks;
		looks.clear();
		for (const auto& look : kLooks) {
			auto* form = data->LookupForm(look.stat, kFile);
			if (auto* model = form ? form->As<RE::TESBoundObject>() : nullptr) {
				looks.emplace_back(model, &look);
			} else {
				SKSE::log::warn("loading screens: {}'s {:03X} is {}", kFile, look.stat,
					form ? std::format("a {} ({:08X}), not a bound object", RE::FormTypeToString(form->GetFormType()), form->GetFormID()) : std::string("missing"));
			}
		}
		if (looks.empty()) {
			SKSE::log::warn("loading screens: none of {}'s models found", kFile);
			return;
		}
		// The camera move most of them have.
		std::map<std::string, int> paths;
		for (auto* screen : data->GetFormArray<RE::TESLoadScreen>()) {
			if (screen && screen->loadNIFData) {
				const char* path = screen->loadNIFData->cameraPath.GetModel();
				++paths[path ? path : ""];
			}
		}
		const auto common = std::ranges::max_element(paths, {}, [](const auto& a_entry) { return a_entry.second; });
		g_cameraPath = common != paths.end() ? common->first : std::string();
		for (const auto& [path, count] : paths) {
			SKSE::log::info("loading screens: {} of the game's have the camera move '{}'", count, path);
		}
		const bool log = Settings::Get().logJumps;  // (the game's own, once, to fit ours by)
		std::size_t changed = 0, withoutModel = 0;
		for (auto* screen : data->GetFormArray<RE::TESLoadScreen>()) {
			if (!screen) {
				continue;
			}
			auto* nif = screen->loadNIFData;
			if (log && changed + withoutModel < 24) {
				const auto [model, text] = Describe(screen);
				SKSE::log::info("loading screens: the game's {:08X}: '{}' scale {:.2f}, turn {} {} {}, offset {:.0f} {:.0f} {:.0f}, limits {} {}: '{:.60}'",
					screen->GetFormID(), model, nif ? nif->initialScale : 0.0f, nif ? nif->rotationConstraints[0] : 0, nif ? nif->rotationConstraints[1] : 0,
					nif ? nif->rotationConstraints[2] : 0, nif ? nif->initialTranslationOffset[0] : 0.0f, nif ? nif->initialTranslationOffset[1] : 0.0f,
					nif ? nif->initialTranslationOffset[2] : 0.0f, nif ? nif->rotationOffsetConstraints[0] : 0, nif ? nif->rotationOffsetConstraints[1] : 0, text);
			}
			if (!nif) {
				++withoutModel;
				continue;
			}
			const auto& [model, look] = looks[changed % looks.size()];
			Apply(nif, model, *look);
			screen->loadingText = kTips[changed % std::size(kTips)];
			++changed;
		}
		SKSE::log::info("loading screens: {} of Skyrim's show Rift Apart's ({} models, {} tips); {} have no model", changed, looks.size(), std::size(kTips),
			withoutModel);
	}
}
