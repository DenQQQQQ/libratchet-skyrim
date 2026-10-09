// SPDX-License-Identifier: GPL-3.0-or-later
#include "CoSave.h"

#include "Bolts.h"
#include "Items.h"

namespace CoSave
{
	namespace
	{
		// The plugin's, and its records: each kept at its own version.
		constexpr std::uint32_t kUniqueID = 'RTCH';
		constexpr std::uint32_t kGiven = 'GIVN';  // u8: this game has had the guns
		constexpr std::uint32_t kBolts = 'BOLT';  // u32: his bolts
		constexpr std::uint32_t kVersion = 1;

		void Save(SKSE::SerializationInterface* a_intfc)
		{
			const std::uint8_t  given = Items::Given() ? 1 : 0;
			const std::uint32_t bolts = Bolts::Total();
			if (!a_intfc->OpenRecord(kGiven, kVersion) || !a_intfc->WriteRecordData(given)) {
				SKSE::log::warn("co-save: couldn't keep that the guns were given");
			}
			if (!a_intfc->OpenRecord(kBolts, kVersion) || !a_intfc->WriteRecordData(bolts)) {
				SKSE::log::warn("co-save: couldn't keep his {} bolts", bolts);
			}
		}

		void Load(SKSE::SerializationInterface* a_intfc)
		{
			std::uint32_t type = 0, version = 0, length = 0;
			while (a_intfc->GetNextRecordInfo(type, version, length)) {
				if (version != kVersion) {
					continue;
				}
				if (type == kGiven) {
					std::uint8_t given = 0;
					if (a_intfc->ReadRecordData(given)) {
						Items::SetGiven(given != 0);
					}
				} else if (type == kBolts) {
					std::uint32_t bolts = 0;
					if (a_intfc->ReadRecordData(bolts)) {
						Bolts::SetTotal(bolts);
					}
				}
			}
			SKSE::log::info("co-save: this save has {}had the guns, and {} bolts", Items::Given() ? "" : "not ", Bolts::Total());
		}

		// A new game, or before a load (a save from before the plugin has no records).
		void Revert(SKSE::SerializationInterface*)
		{
			Items::SetGiven(false);
			Bolts::SetTotal(0);
		}
	}

	void Register()
	{
		auto* serialization = SKSE::GetSerializationInterface();
		serialization->SetUniqueID(kUniqueID);
		serialization->SetSaveCallback(Save);
		serialization->SetLoadCallback(Load);
		serialization->SetRevertCallback(Revert);
	}
}
