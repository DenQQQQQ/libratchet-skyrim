// SPDX-License-Identifier: GPL-3.0-or-later
#include "Spells.h"

namespace Spells
{
	namespace
	{
		constexpr RE::FormID kDamageHealth = 0x0003EB42;  // AlchDamageHealth
		constexpr RE::FormID kStagger = 0x0007809B;       // StaggerAttackFFContact
		constexpr RE::FormID kBolt = 0x00012E84;          // FireboltProjectile01

		RE::SpellItem* g_hit = nullptr;      // health damage on the target (the magnitude per cast)
		RE::SpellItem* g_stagger = nullptr;  // a stagger (the magnitude per cast)
		RE::SpellItem* g_shot = nullptr;     // a bolt that does health damage where it lands

		template <class T>
		T* New()
		{
			auto* factory = RE::IFormFactory::GetConcreteFormFactoryByType<T>();
			return factory ? factory->Create() : nullptr;
		}

		// Damage that comes from Ratchet, not from alchemy: no resistance, no skill.
		RE::EffectSetting* Damage(RE::EffectSetting* a_vanilla, RE::MagicSystem::Delivery a_delivery, RE::BGSProjectile* a_projectile)
		{
			auto* e = New<RE::EffectSetting>();
			if (!e) {
				return nullptr;
			}
			e->Copy(a_vanilla);
			e->data.resistVariable = RE::ActorValue::kNone;
			e->data.associatedSkill = RE::ActorValue::kNone;
			e->data.skillUsageMult = 0.0f;
			e->data.minimumSkill = 0;
			e->data.castingType = RE::MagicSystem::CastingType::kFireAndForget;
			e->data.delivery = a_delivery;
			e->data.projectileBase = a_projectile;
			e->data.flags.reset(RE::EffectSetting::EffectSettingData::Flag::kRecover);
			return e;
		}

		RE::SpellItem* Spell(RE::EffectSetting* a_effect, float a_magnitude, RE::MagicSystem::Delivery a_delivery)
		{
			auto* spell = New<RE::SpellItem>();
			if (!spell || !a_effect) {
				return nullptr;
			}
			spell->data.spellType = RE::MagicSystem::SpellType::kSpell;
			spell->data.castingType = RE::MagicSystem::CastingType::kFireAndForget;
			spell->data.delivery = a_delivery;
			spell->data.costOverride = 0;
			spell->data.flags.set(RE::SpellItem::SpellFlag::kCostOverride, RE::SpellItem::SpellFlag::kIgnoreResistance,
				RE::SpellItem::SpellFlag::kNoAbsorb, RE::SpellItem::SpellFlag::kNoDualCastMods);
			auto* effect = new RE::Effect();
			effect->baseEffect = a_effect;
			effect->effectItem.magnitude = a_magnitude;
			effect->effectItem.area = 0;
			effect->effectItem.duration = 0;
			effect->cost = 0.0f;
			spell->effects.push_back(effect);
			return spell;
		}
	}

	bool Build()
	{
		auto* damage = RE::TESForm::LookupByID<RE::EffectSetting>(kDamageHealth);
		auto* stagger = RE::TESForm::LookupByID<RE::EffectSetting>(kStagger);
		auto* bolt = RE::TESForm::LookupByID<RE::BGSProjectile>(kBolt);
		if (!damage || !stagger || !bolt) {
			SKSE::log::error("combat: vanilla records missing (damage {}, stagger {}, bolt {}): no hits", damage != nullptr, stagger != nullptr, bolt != nullptr);
			return false;
		}
		using Delivery = RE::MagicSystem::Delivery;
		g_hit = Spell(Damage(damage, Delivery::kTargetActor, nullptr), 0.0f, Delivery::kTargetActor);
		g_stagger = Spell(stagger, 0.0f, Delivery::kTargetActor);
		g_shot = Spell(Damage(damage, Delivery::kAimed, bolt), 0.0f, Delivery::kAimed);
		SKSE::log::info("combat: spells built (hit {:08X}, stagger {:08X}, shot {:08X})", g_hit ? g_hit->GetFormID() : 0,
			g_stagger ? g_stagger->GetFormID() : 0, g_shot ? g_shot->GetFormID() : 0);
		return g_hit && g_stagger && g_shot;
	}

	void Hit(RE::Actor* a_target, float a_damage, float a_stagger)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		if (!player || !a_target || a_target == player || a_target->IsDead()) {
			return;
		}
		auto* caster = player->GetMagicCaster(RE::MagicSystem::CastingSource::kInstant);
		if (!caster) {
			return;
		}
		if (g_hit && a_damage > 0.0f) {
			caster->CastSpellImmediate(g_hit, false, a_target, 1.0f, false, a_damage, player);
		}
		if (g_stagger && a_stagger > 0.0f) {
			caster->CastSpellImmediate(g_stagger, true, a_target, 1.0f, false, a_stagger, player);
		}
	}

	void Shoot(const RE::NiPoint3& a_from, const RE::NiPoint3& a_dir, float a_damage)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		if (!player || !g_shot || g_shot->effects.empty()) {
			return;
		}
		g_shot->effects[0]->effectItem.magnitude = a_damage;
		// Skyrim's projectile angles: x pitches (positive is down), z turns from north.
		const RE::Projectile::ProjectileRot angles{ -std::asin(std::clamp(a_dir.z, -1.0f, 1.0f)), std::atan2(a_dir.x, a_dir.y) };
		RE::ProjectileHandle handle;
		RE::Projectile::LaunchSpell(&handle, player, g_shot, a_from, angles);
	}
}
