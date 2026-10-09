// SPDX-License-Identifier: GPL-3.0-or-later
#include "Items.h"

namespace Items
{
	namespace
	{
		// RatchetMovement.esl's records (skse/tools/ratchet_plugin.py).
		constexpr std::string_view kFile = "RatchetMovement.esl";
		constexpr RE::FormID       kPistol = 0x800;
		constexpr RE::FormID       kShatterbomb = 0x801;

		RE::TESObjectWEAP* g_pistol = nullptr;
		RE::TESObjectWEAP* g_shatterbomb = nullptr;
		bool               g_given = false;

		RE::TESObjectWEAP* Weapon(Combat::Gun a_gun)
		{
			return a_gun == Combat::Gun::kBurstPistol ? g_pistol : a_gun == Combat::Gun::kShatterbomb ? g_shatterbomb : nullptr;
		}
	}

	bool Install()
	{
		auto* data = RE::TESDataHandler::GetSingleton();
		if (!data || !data->LookupModByName(kFile)) {
			SKSE::log::info("items: no {} loaded: the guns are picked on Combat's own keys", kFile);
			return false;
		}
		g_pistol = data->LookupForm<RE::TESObjectWEAP>(kPistol, kFile);
		g_shatterbomb = data->LookupForm<RE::TESObjectWEAP>(kShatterbomb, kFile);
		SKSE::log::info("items: {}: the Burst Pistol {:08X}, the Shatterbomb {:08X}", kFile, g_pistol ? g_pistol->GetFormID() : 0,
			g_shatterbomb ? g_shatterbomb->GetFormID() : 0);
		return Available();
	}

	bool Available() { return g_pistol && g_shatterbomb; }

	bool Given() { return g_given; }

	void SetGiven(bool a_given) { g_given = a_given; }

	int Count(Combat::Gun a_gun)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* weapon = Weapon(a_gun);
		if (!player || !weapon) {
			return 0;
		}
		const auto counts = player->GetInventoryCounts();
		const auto it = counts.find(weapon);
		return it != counts.end() ? it->second : 0;
	}

	void GiveOnce()
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		if (g_given || !Available() || !player) {
			return;
		}
		g_given = true;
		for (const auto gun : { Combat::Gun::kBurstPistol, Combat::Gun::kShatterbomb }) {
			if (Count(gun) < 1) {
				auto* weapon = Weapon(gun);
				player->AddObjectToContainer(weapon, nullptr, 1, nullptr);
				RE::SendHUDMessage::ShowInventoryChangeMessage(weapon, 1, true, false);
				SKSE::log::info("items: the {} put in his inventory (once a game)", weapon->GetName());
			}
		}
	}

	std::optional<Combat::Gun> Equipped()
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		if (!Available() || !player) {
			return std::nullopt;
		}
		for (const bool left : { false, true }) {
			const auto* object = player->GetEquippedObject(left);
			if (object && object == g_pistol) {
				return Combat::Gun::kBurstPistol;
			}
			if (object && object == g_shatterbomb) {
				return Combat::Gun::kShatterbomb;
			}
		}
		return std::nullopt;
	}

	void Equip(Combat::Gun a_gun)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* manager = RE::ActorEquipManager::GetSingleton();
		if (!Available() || !player || !manager) {
			return;
		}
		if (auto* weapon = Weapon(a_gun)) {
			if (Equipped() == a_gun) {
				return;
			}
			if (Count(a_gun) < 1) {
				player->AddObjectToContainer(weapon, nullptr, 1, nullptr);
			}
			manager->EquipObject(player, weapon);
			return;
		}
		for (auto* weapon : { g_pistol, g_shatterbomb }) {
			if (player->GetEquippedObject(false) == weapon || player->GetEquippedObject(true) == weapon) {
				manager->UnequipObject(player, weapon);
			}
		}
	}
}
