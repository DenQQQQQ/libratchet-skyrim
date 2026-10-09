// SPDX-License-Identifier: GPL-3.0-or-later
#include "Hud.h"

namespace Hud
{
	namespace
	{
		// The sandbox's counter (bolts.rs, its 1280 x 720 window): 16 px from the top and 28 px from
		// the right; the total at 30 px in a warm white, the "+N" under it at 24 px in gold, each
		// with a shadow 2 px down and right (black at 85%). Skyrim's HUD movie has the same 1280 x
		// 720 stage. The font is Skyrim's bold one.
		constexpr double        kTop = 16.0;
		constexpr double        kRight = 28.0;
		constexpr double        kTotalSize = 30.0;
		constexpr double        kGainSize = 24.0;
		constexpr std::uint32_t kTotalColor = 0xFFF2D9;  // (1, 0.95, 0.85)
		constexpr std::uint32_t kGainColor = 0xFFD159;   // (1, 0.82, 0.35)
		constexpr double        kShadowOffset = 2.0;
		constexpr double        kShadowAlpha = 85.0;
		constexpr double        kWidth = 480.0;
		constexpr const char*   kFont = "$EverywhereBoldFont";

		// What the counter's fields say, for the tests (set on the UI thread).
		std::mutex                          g_textLock;
		std::pair<std::string, std::string> g_text;

		// 1234567 -> "1,234,567".
		std::string Thousands(std::uint32_t a_n)
		{
			const std::string digits = std::to_string(a_n);
			std::string       out;
			for (std::size_t i = 0; i < digits.size(); ++i) {
				if (i > 0 && (digits.size() - i) % 3 == 0) {
					out += ',';
				}
				out += digits[i];
			}
			return out;
		}

		// Our text field a_name on the HUD's root, made the first time (Skyrim's HUD movie may be
		// made again: then it's made again too).
		bool Field(RE::GFxMovieView* a_movie, RE::GFxValue& a_root, const char* a_name, double a_size, std::uint32_t a_color, double a_alpha,
			RE::GFxValue& a_out)
		{
			if (a_root.GetMember(a_name, &a_out) && a_out.IsDisplayObject()) {
				return true;
			}
			RE::GFxValue depth;
			a_root.Invoke("getNextHighestDepth", &depth);
			const std::array<RE::GFxValue, 6> args{ RE::GFxValue(a_name), depth, RE::GFxValue(0.0), RE::GFxValue(0.0), RE::GFxValue(kWidth),
				RE::GFxValue(a_size * 1.6) };
			a_root.Invoke("createTextField", nullptr, args);
			if (!a_root.GetMember(a_name, &a_out) || !a_out.IsDisplayObject()) {
				return false;
			}
			RE::GFxValue format;
			a_movie->CreateObject(&format, "TextFormat");
			format.SetMember("font", RE::GFxValue(kFont));
			format.SetMember("size", RE::GFxValue(a_size));
			format.SetMember("color", RE::GFxValue(static_cast<double>(a_color)));
			format.SetMember("align", RE::GFxValue("right"));
			a_out.SetMember("embedFonts", RE::GFxValue(true));
			a_out.SetMember("selectable", RE::GFxValue(false));
			a_out.SetMember("_alpha", RE::GFxValue(a_alpha));
			a_out.Invoke("setNewTextFormat", nullptr, &format, 1);
			SKSE::log::info("hud: {} made on the HUD", a_name);
			return true;
		}

		void Set(RE::GFxValue& a_field, const std::string& a_text, bool a_visible, double a_x, double a_y)
		{
			a_field.SetText(a_text.c_str());
			a_field.SetMember("_x", RE::GFxValue(a_x));
			a_field.SetMember("_y", RE::GFxValue(a_y));
			a_field.SetMember("_visible", RE::GFxValue(a_visible && !a_text.empty()));
		}
	}

	void BoltCounter(bool a_visible, std::uint32_t a_shown, std::uint32_t a_gain)
	{
		auto* tasks = SKSE::GetTaskInterface();
		if (!tasks) {
			return;
		}
		std::string total = std::format("BOLTS  {}", Thousands(a_shown));
		std::string gain = a_gain > 0 ? std::format("+{}", Thousands(a_gain)) : std::string();
		tasks->AddUITask([a_visible, total = std::move(total), gain = std::move(gain)] {
			auto*      ui = RE::UI::GetSingleton();
			const auto movie = ui ? ui->GetMovieView(RE::HUDMenu::MENU_NAME) : nullptr;
			RE::GFxValue root;
			if (!movie || !movie->GetVariable(&root, "_root")) {
				return;
			}
			// Shadows first: under their text.
			RE::GFxValue totalShadow, gainShadow, totalField, gainField;
			if (!Field(movie.get(), root, "RatchetBoltsTotalShadow", kTotalSize, 0x000000, kShadowAlpha, totalShadow) ||
				!Field(movie.get(), root, "RatchetBoltsGainShadow", kGainSize, 0x000000, kShadowAlpha, gainShadow) ||
				!Field(movie.get(), root, "RatchetBoltsTotal", kTotalSize, kTotalColor, 100.0, totalField) ||
				!Field(movie.get(), root, "RatchetBoltsGain", kGainSize, kGainColor, 100.0, gainField)) {
				static bool warned = false;
				if (!warned) {
					warned = true;
					SKSE::log::warn("hud: couldn't make the bolt counter's text fields on the HUD");
				}
				return;
			}
			// From the right edge of what's on screen (wider screens show more of the stage).
			const auto   rect = movie->GetVisibleFrameRect();
			const double x = rect.right - kRight - kWidth;
			const double y = rect.top + kTop;
			const double gainY = y + kTotalSize * 1.25;
			Set(totalShadow, total, a_visible, x + kShadowOffset, y + kShadowOffset);
			Set(totalField, total, a_visible, x, y);
			Set(gainShadow, gain, a_visible, x + kShadowOffset, gainY + kShadowOffset);
			Set(gainField, gain, a_visible, x, gainY);
			// What the fields say now, as Scaleform has it.
			const auto said = [](RE::GFxValue& a_field) {
				RE::GFxValue text, shown;
				a_field.GetMember("_visible", &shown);
				const bool up = shown.IsBool() && shown.GetBool();
				return up && a_field.GetMember("text", &text) && text.IsString() ? std::string(text.GetString()) : std::string();
			};
			std::scoped_lock lock(g_textLock);
			g_text = { said(totalField), said(gainField) };
		});
	}

	std::pair<std::string, std::string> BoltCounterText()
	{
		std::scoped_lock lock(g_textLock);
		return g_text;
	}
}
