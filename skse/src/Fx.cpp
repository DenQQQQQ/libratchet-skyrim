// SPDX-License-Identifier: GPL-3.0-or-later
#include "Fx.h"

#include "Settings.h"

namespace Fx
{
	namespace
	{
		float Units() { return RE::bhkWorld::GetWorldScaleInverse(); }
		// A metre at his size, in game units: the sandbox's sizes and speeds are metres × scale.
		float M() { return Settings::Get().scale * Units(); }

		constexpr float kPi = 3.14159265f;
		constexpr float kTau = 2.0f * kPi;
		const RE::NiPoint3 kUp{ 0.0f, 0.0f, 1.0f };
		// How long a bolt's streak takes to catch up with it once it has hit something (seconds).
		constexpr float kStreakCatchUp = 0.05f;
		// Marks lie this far off what they're on, game units (1 is 1.4 cm). The sandbox's 4-10 mm
		// is lost in Skyrim's depth buffer a few metres away.
		constexpr float kMarkLift = 1.0f;

		struct Color
		{
			float r = 1.0f, g = 1.0f, b = 1.0f;
			Color operator*(float a_k) const { return { r * a_k, g * a_k, b * a_k }; }
			Color Mix(const Color& a_to, float a_t) const { return { r + (a_to.r - r) * a_t, g + (a_to.g - g) * a_t, b + (a_to.b - b) * a_t }; }
		};

		// The sandbox's colours (linear RGB, past 1 to glow): its own unless the converter's
		// palette has the game's (fx.ini, from vfx.json).
		struct Palette
		{
			Color bolt{ 0.1f, 0.8f, 0.1f };
			Color boltGlow{ 0.4f, 0.9f, 0.1f };
			Color impactFlash{ 0.9f, 1.0f, 0.4f };
			Color impactSpark{ 0.7f, 1.0f, 0.3f };
			Color impactGlow{ 0.4f, 0.9f, 0.2f };
			Color impactGlowEnd{ 0.15f, 0.25f, 0.1f };
			Color impactSmoke{ 0.25f, 0.25f, 0.25f };
			Color decalGlow{ 0.2f, 0.8f, 0.05f };
			Color shellGlass{ 0.1f, 0.2f, 0.05f };
			Color blastHot{ 1.5f, 1.3f, 1.0f };
			Color blastFire{ 1.0f, 0.6f, 0.25f };
			Color blastFireEnd{ 1.0f, 0.35f, 0.05f };
			Color blastEnergy{ 0.5f, 0.3f, 1.0f };
			Color blastShards{ 0.8f, 0.1f, 1.0f };
			Color blastSmoke{ 0.2f, 0.22f, 0.25f };
			Color scorchGlow{ 0.3f, 0.1f, 0.8f };
			Color grenadeGlow{ 0.45f, 0.1f, 0.85f };
			Color wrenchHit{ 0.2f, 0.65f, 1.0f };
			Color wrenchSmoke{ 0.3f, 0.33f, 0.35f };
			Color wrenchSlam{ 0.35f, 0.75f, 1.0f };
			Color wrenchSlamGlow{ 0.2f, 0.7f, 1.0f };
			Color slamScorch{ 0.2f, 0.7f, 1.0f };
			Color wrenchTrail{ 0.5f, 0.85f, 1.0f };
			Color wrenchTrailEnd{ 0.05f, 0.6f, 1.0f };
		};
		Palette g_c;

		void LoadPalette()
		{
			const auto ini = PluginDirectory() / "RatchetMovement" / "fx.ini";
			int        fromGame = 0;
			const auto read = [&](const wchar_t* a_key, Color& a_color) {
				wchar_t buf[128]{};
				GetPrivateProfileStringW(L"Palette", a_key, L"", buf, static_cast<DWORD>(std::size(buf)), ini.c_str());
				float rgb[3]{};
				if (std::swscanf(buf, L"%f,%f,%f", &rgb[0], &rgb[1], &rgb[2]) == 3) {
					a_color = { rgb[0], rgb[1], rgb[2] };
					++fromGame;
				}
			};
			const auto intensity = [&](const wchar_t* a_key, float a_default) {
				wchar_t buf[64]{};
				GetPrivateProfileStringW(L"Palette", a_key, L"", buf, static_cast<DWORD>(std::size(buf)), ini.c_str());
				float v = 0.0f;
				return std::swscanf(buf, L"%f", &v) == 1 ? v : a_default;
			};
			read(L"bolt", g_c.bolt);
			read(L"bolt_glow", g_c.boltGlow);
			read(L"impact_flash", g_c.impactFlash);
			read(L"impact_spark", g_c.impactSpark);
			read(L"impact_glow", g_c.impactGlow);
			read(L"impact_glow_end", g_c.impactGlowEnd);
			read(L"impact_smoke", g_c.impactSmoke);
			read(L"decal_glow", g_c.decalGlow);
			read(L"shell_glass", g_c.shellGlass);
			read(L"blast_hot", g_c.blastHot);
			read(L"blast_fire", g_c.blastFire);
			read(L"blast_fire_end", g_c.blastFireEnd);
			read(L"blast_energy", g_c.blastEnergy);
			read(L"blast_shards", g_c.blastShards);
			read(L"blast_smoke", g_c.blastSmoke);
			read(L"scorch_glow", g_c.scorchGlow);
			read(L"grenade_glow", g_c.grenadeGlow);
			read(L"wrench_hit", g_c.wrenchHit);
			read(L"wrench_smoke", g_c.wrenchSmoke);
			read(L"wrench_slam", g_c.wrenchSlam);
			read(L"wrench_slam_glow", g_c.wrenchSlamGlow);
			read(L"slam_scorch", g_c.slamScorch);
			read(L"wrench_trail", g_c.wrenchTrail);
			read(L"wrench_trail_end", g_c.wrenchTrailEnd);
			g_c.bolt = g_c.bolt * intensity(L"bolt_intensity", 5.0f);
			SKSE::log::info("fx: {} colours from the game's effect data", fromGame);
		}

		// Effect variety, as the sandbox's FxRng (deterministic).
		struct Rng
		{
			std::uint32_t x = 0x2545F491u;
			float Next()
			{
				x ^= x << 13;
				x ^= x >> 17;
				x ^= x << 5;
				return static_cast<float>(x >> 8) / static_cast<float>(1u << 24);
			}
			float        Signed() { return Next() * 2.0f - 1.0f; }
			RE::NiPoint3 Unit()
			{
				RE::NiPoint3 v{ Signed(), Signed(), Signed() };
				const float  l = v.Length();
				return l > 1e-4f ? v / l : kUp;
			}
		};
		Rng g_rng;

		// --- turns

		RE::NiMatrix3 Columns(const RE::NiPoint3& a_x, const RE::NiPoint3& a_y, const RE::NiPoint3& a_z)
		{
			RE::NiMatrix3 m;
			m.entry[0][0] = a_x.x;
			m.entry[1][0] = a_x.y;
			m.entry[2][0] = a_x.z;
			m.entry[0][1] = a_y.x;
			m.entry[1][1] = a_y.y;
			m.entry[2][1] = a_y.z;
			m.entry[0][2] = a_z.x;
			m.entry[1][2] = a_z.y;
			m.entry[2][2] = a_z.z;
			return m;
		}

		RE::NiMatrix3 AboutZ(float a_angle)
		{
			const float c = std::cos(a_angle), s = std::sin(a_angle);
			return Columns({ c, s, 0.0f }, { -s, c, 0.0f }, kUp);
		}

		// A turn of |a_v| radians about a_v.
		RE::NiMatrix3 AxisAngle(const RE::NiPoint3& a_v)
		{
			const float angle = a_v.Length();
			if (angle < 1e-6f) {
				return Columns({ 1.0f, 0.0f, 0.0f }, { 0.0f, 1.0f, 0.0f }, kUp);
			}
			const RE::NiPoint3 k = a_v / angle;
			const float        c = std::cos(angle), s = std::sin(angle), t = 1.0f - c;
			RE::NiMatrix3      m;
			m.entry[0][0] = c + t * k.x * k.x;
			m.entry[0][1] = t * k.x * k.y - s * k.z;
			m.entry[0][2] = t * k.x * k.z + s * k.y;
			m.entry[1][0] = t * k.x * k.y + s * k.z;
			m.entry[1][1] = c + t * k.y * k.y;
			m.entry[1][2] = t * k.y * k.z - s * k.x;
			m.entry[2][0] = t * k.x * k.z - s * k.y;
			m.entry[2][1] = t * k.y * k.z + s * k.x;
			m.entry[2][2] = c + t * k.z * k.z;
			return m;
		}

		// The shortest turn taking its z axis to a_dir (unit): the gun's flash and bolt shapes
		// point their z along the shot, a mark faces out of the wall.
		RE::NiMatrix3 ZTo(const RE::NiPoint3& a_dir)
		{
			const float c = std::clamp(a_dir.z, -1.0f, 1.0f);
			if (c < -0.9999f) {
				return Columns({ 1.0f, 0.0f, 0.0f }, { 0.0f, -1.0f, 0.0f }, { 0.0f, 0.0f, -1.0f });
			}
			RE::NiPoint3 axis = kUp.Cross(a_dir);
			const float  s = axis.Length();
			if (s < 1e-6f) {
				return AxisAngle({});
			}
			return AxisAngle(axis * (std::atan2(s, c) / s));
		}

		// The game's meshes are glTF's, y up: the sandbox turns them in its y-up world. The same
		// turn in Skyrim's z-up world: (x, y, z) -> (x, -z, y) after it.
		RE::NiMatrix3 YUp(const RE::NiMatrix3& a_sandbox)
		{
			const RE::NiMatrix3 m = Columns({ 1.0f, 0.0f, 0.0f }, { 0.0f, 0.0f, 1.0f }, { 0.0f, -1.0f, 0.0f });
			return m * a_sandbox;
		}

		// Its long axis (y) along a_axis, its face (z) turned as far toward the camera as that
		// allows (the sandbox's axis_facing).
		RE::NiMatrix3 AxisFacing(const RE::NiPoint3& a_axis, const RE::NiPoint3& a_toCamera)
		{
			RE::NiPoint3 z = a_toCamera - a_axis * a_toCamera.Dot(a_axis);
			float        l = z.Length();
			if (l < 1e-5f) {
				z = std::abs(a_axis.z) < 0.9f ? a_axis.UnitCross(kUp) : a_axis.UnitCross({ 1.0f, 0.0f, 0.0f });
				l = 1.0f;
			}
			z = z / l;
			return Columns(a_axis.Cross(z), a_axis, z);
		}

		struct View
		{
			RE::NiPoint3  at;
			RE::NiMatrix3 facing;  // a quad's turn to face the camera: x right, y up, z back to it
		};

		View CameraView()
		{
			View        view{ {}, AxisAngle({}) };
			const auto* camera = RE::PlayerCamera::GetSingleton();
			if (camera && camera->cameraRoot) {
				const auto& w = camera->cameraRoot->world;
				view.at = w.translate;
				const RE::NiPoint3 forward = w.rotate.GetVectorY();
				view.facing = Columns(w.rotate.GetVectorX(), w.rotate.GetVectorZ(), forward * -1.0f);
			}
			return view;
		}

		// --- the pieces' templates

		struct Template
		{
			RE::NiPointer<RE::NiAVObject>              node;
			std::vector<RE::NiPointer<RE::NiAVObject>> spare;  // copies done with, culled
			bool                                       failed = false;
		};
		std::unordered_map<std::string, Template> g_templates;
		RE::NiPointer<RE::NiNode>                 g_root;
		int                                       g_available = -1;  // not looked yet

		// Under the player's cell's 3D, as the cell's own objects are: drawn in first and third
		// person alike. (Hung under Skyrim's shadow scene node directly, nothing was drawn.)
		RE::NiNode* Root()
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* cell = player ? player->GetParentCell() : nullptr;
			auto* loaded = cell ? cell->GetRuntimeData().loadedData : nullptr;
			auto* holder = loaded ? loaded->cell3D.get() : nullptr;
			if (!holder) {
				return nullptr;
			}
			if (!g_root) {
				g_root.reset(RE::NiNode::Create(0));
				g_root->name = "RatchetFx";
			}
			if (g_root->parent != holder) {
				if (g_root->parent) {
					g_root->parent->DetachChild(g_root.get());
				}
				holder->AttachChild(g_root.get(), true);
				const auto& w = holder->world;
				SKSE::log::info("fx: under {}'s 3D (at {:.1f} {:.1f} {:.1f}, scale {:.2f})", cell->GetFormEditorID(), w.translate.x, w.translate.y,
					w.translate.z, w.scale);
			}
			// Its transform undoes the cell node's: the pieces' are the world's.
			const auto&         w = holder->world;
			const RE::NiMatrix3 back = w.rotate.Transpose();
			const float         k = w.scale > 1e-6f ? 1.0f / w.scale : 1.0f;
			g_root->local.rotate = back;
			g_root->local.scale = k;
			g_root->local.translate = (back * w.translate) * -k;
			return g_root.get();
		}

		RE::BSEffectShaderProperty* ShaderOf(RE::NiAVObject* a_node)
		{
			if (!a_node) {
				return nullptr;
			}
			if (auto* geometry = a_node->AsGeometry()) {
				return netimmerse_cast<RE::BSEffectShaderProperty*>(geometry->GetGeometryRuntimeData().shaderProperty.get());
			}
			if (auto* node = a_node->AsNode()) {
				for (auto& child : node->GetChildren()) {
					if (auto* found = ShaderOf(child.get())) {
						return found;
					}
				}
			}
			return nullptr;
		}

		// A copy's own material: the template's is shared by every copy (and by Skyrim's cache).
		void OwnMaterial(RE::BSEffectShaderProperty* a_shader)
		{
			auto* material = a_shader ? static_cast<RE::BSEffectShaderMaterial*>(a_shader->material) : nullptr;
			if (!material) {
				return;
			}
			auto* copy = static_cast<RE::BSEffectShaderMaterial*>(material->Create());
			if (!copy) {
				return;
			}
			copy->CopyMembers(material);
			a_shader->SetMaterial(copy, true);
			static bool logged = false;
			if (!logged) {
				logged = true;
				SKSE::log::info("fx: a copy's material is {} (the template's {:p}, ours {:p}, now {:p})",
					a_shader->material != material ? "its own" : "STILL SHARED", static_cast<void*>(material), static_cast<void*>(copy),
					static_cast<void*>(a_shader->material));
			}
		}

		bool Load(Template& a_template, const std::string& a_name)
		{
			if (!a_template.node && !a_template.failed) {
				RE::NiPointer<RE::NiNode>              loaded;
				const RE::BSModelDB::DBTraits::ArgsType args{};
				const auto path = std::format("RatchetMovement\\fx\\{}.nif", a_name);
				const auto err = RE::BSModelDB::Demand(path.c_str(), loaded, args);
				if (err != RE::BSResource::ErrorCode::kNone || !loaded) {
					SKSE::log::warn("fx: no meshes\\{} (error {})", path, static_cast<int>(err));
					a_template.failed = true;
					return false;
				}
				a_template.node = loaded;
			}
			return a_template.node != nullptr;
		}

		// One shape of a piece.
		struct Part
		{
			RE::NiPointer<RE::NiAVObject> node;
			RE::BSEffectShaderProperty*   shader = nullptr;
			std::string                   name;          // its template's
			bool                          add = false;   // added light: fades by dimming, not by transparency
			Color                         color;
			float                         alpha = 1.0f;  // times the piece's
		};

		Part Make(std::string a_name, bool a_add, const Color& a_color, float a_alpha = 1.0f)
		{
			Part part{ nullptr, nullptr, std::move(a_name), a_add, a_color, a_alpha };
			auto& t = g_templates[part.name];
			auto* root = Root();
			if (!root || !Load(t, part.name)) {
				return part;
			}
			if (!t.spare.empty()) {
				part.node = t.spare.back();
				t.spare.pop_back();
			} else {
				auto* copy = t.node->Clone();
				auto* object = copy ? netimmerse_cast<RE::NiAVObject*>(copy) : nullptr;
				if (!object) {
					return part;
				}
				part.node.reset(object);
				part.node->SetAppCulled(true);
				root->AttachChild(object, true);
				OwnMaterial(ShaderOf(object));
			}
			part.shader = ShaderOf(part.node.get());
			return part;
		}

		void Release(Part& a_part)
		{
			if (!a_part.node) {
				return;
			}
			a_part.node->SetAppCulled(true);
			g_templates[a_part.name].spare.push_back(a_part.node);
			a_part.node.reset();
			a_part.shader = nullptr;
		}

		// Where a shape is, its turn and its size along each of its axes (game units). Skyrim's
		// transforms have one scale: a stretched shape carries its stretch in its turn, its scale
		// the largest (which its bounds follow).
		void Place(Part& a_part, const RE::NiPoint3& a_at, const RE::NiMatrix3& a_turn, const RE::NiPoint3& a_size)
		{
			if (!a_part.node) {
				return;
			}
			const float s = std::max({ std::abs(a_size.x), std::abs(a_size.y), std::abs(a_size.z) });
			if (s < 1e-3f) {
				if (!a_part.node->GetAppCulled()) {
					a_part.node->SetAppCulled(true);
				}
				return;
			}
			RE::NiMatrix3 m = a_turn;
			for (int r = 0; r < 3; ++r) {
				m.entry[r][0] *= a_size.x / s;
				m.entry[r][1] *= a_size.y / s;
				m.entry[r][2] *= a_size.z / s;
			}
			a_part.node->local.translate = a_at;
			a_part.node->local.rotate = m;
			a_part.node->local.scale = s;
			if (a_part.node->GetAppCulled()) {
				a_part.node->SetAppCulled(false);
			}
		}

		// Skyrim's effect colours are in display (gamma) space; the sandbox's are linear.
		float Gamma(float a_v) { return a_v > 0.0f ? std::pow(a_v, 1.0f / 2.2f) : 0.0f; }

		void Tint(Part& a_part, const Color& a_color, float a_alpha)
		{
			auto* material = a_part.shader ? static_cast<RE::BSEffectShaderMaterial*>(a_part.shader->material) : nullptr;
			if (!material) {
				return;
			}
			const float a = std::clamp(a_alpha * a_part.alpha, 0.0f, 1.0f);
			const float k = a_part.add ? a : 1.0f;
			material->baseColor.red = Gamma(a_color.r) * k;
			material->baseColor.green = Gamma(a_color.g) * k;
			material->baseColor.blue = Gamma(a_color.b) * k;
			material->baseColor.alpha = a;
		}

		// --- the pieces

		enum class Facing
		{
			kFixed,     // its own turn (and spin)
			kCamera,    // faces the camera
			kVelocity,  // stretched along its velocity, turned to the camera (sparks)
			kAxis,      // stretched along a fixed way, turned to the camera (trails)
		};

		// One piece of an effect (the sandbox's Fx): it moves, grows and fades over its life.
		struct Piece
		{
			std::vector<Part>    parts;  // one shape, or a few moving together (a shell)
			float                age = 0.0f;
			float                life = 1.0f;
			RE::NiPoint3         at;      // game units
			RE::NiPoint3         vel;     // game units/s
			float                gravity = 0.0f;  // game units/s²
			float                drag = 0.0f;
			RE::NiPoint3         spin;    // radians/s about the world's axes
			RE::NiMatrix3        turn = AxisAngle({});
			RE::NiPoint3         size0{ 1.0f, 1.0f, 1.0f };  // game units, along its own axes
			RE::NiPoint3         size1{ 1.0f, 1.0f, 1.0f };
			float                alpha0 = 1.0f;
			float                alpha1 = 0.0f;
			float                fadeFrom = 0.0f;  // the fade starts this far into its life
			std::optional<Color> colorEnd;         // the colour it shifts to
			Facing               facing = Facing::kFixed;
			RE::NiPoint3         axis;
			float                roll = 0.0f;  // camera-facing pieces: about the view
			std::optional<float> floor;        // bounces off this height
			bool                 shrink = false;  // solid pieces shrink away at the end instead
		};
		std::vector<Piece> g_pieces;

		RE::NiPoint3 Splat(float a_v) { return { a_v, a_v, a_v }; }

		void Spawn(Piece&& a_piece)
		{
			if (std::ranges::none_of(a_piece.parts, [](const Part& a_part) { return a_part.node != nullptr; })) {
				for (auto& part : a_piece.parts) {
					Release(part);
				}
				return;
			}
			for (auto& part : a_piece.parts) {
				Tint(part, part.color, a_piece.alpha0);
			}
			g_pieces.push_back(std::move(a_piece));
		}

		// A camera-facing sprite of a texture, growing from a_size.first to a_size.second.
		void Sprite(const char* a_texture, bool a_add, const RE::NiPoint3& a_at, std::pair<float, float> a_size, const Color& a_color, Piece a_piece)
		{
			a_piece.parts.push_back(Make(std::format("quad_{}_{}", a_texture, a_add ? "add" : "blend"), a_add, a_color));
			a_piece.at = a_at;
			a_piece.size0 = Splat(a_size.first);
			a_piece.size1 = Splat(a_size.second);
			a_piece.facing = Facing::kCamera;
			Spawn(std::move(a_piece));
		}

		Piece Life(float a_life)
		{
			Piece p;
			p.life = a_life;
			return p;
		}

		// --- bolts

		struct BoltLook
		{
			std::uint32_t id = 0;
			RE::NiPoint3  origin;
			RE::NiPoint3  head;
			RE::NiPoint3  dir;
			Part          tracer, shape, streak, glow;
			bool          ended = false;
			float         gone = 0.0f;  // since it ended
		};
		std::vector<BoltLook> g_bolts;
		std::uint32_t         g_nextBolt = 1;

		// The throw arc's lines, kept from frame to frame; those not drawn this frame are hidden.
		std::vector<Part> g_arc;
		bool              g_arcDrawn = false;

		void HideArc(std::size_t a_from)
		{
			for (std::size_t i = a_from; i < g_arc.size(); ++i) {
				if (g_arc[i].node && !g_arc[i].node->GetAppCulled()) {
					g_arc[i].node->SetAppCulled(true);
				}
			}
		}

		// --- lights: the sandbox's flash_light, a point light without shadows that fades with its
		// piece (full for the first tenth of its life). Skyrim lights a shape with only a few: at
		// most kMaxLights of ours at once, the oldest going first.
		constexpr std::size_t kMaxLights = 8;

		struct Light
		{
			RE::NiPointer<RE::NiPointLight> light;
			float                           age = 0.0f;
			float                           life = 1.0f;  // < 0: carried (a bolt's, a bomb's) until it isn't placed
			float                           fade = 1.0f;  // Skyrim's, at full
			std::uint32_t                   key = 0;      // what carries it
			bool                            seen = true;  // carried: placed this frame
		};
		std::vector<Light> g_lights;

		// The sandbox's lumens (Bevy's point lights) as Skyrim's fade (ours): a bolt's impact,
		// 40 000 lm, is 1.
		float LightFade(float a_lumens) { return std::clamp(a_lumens / 40000.0f, 0.3f, 5.0f); }

		void DropLight(Light& a_light)
		{
			if (!a_light.light) {
				return;
			}
			if (auto* scene = RE::BSShaderManager::State::GetSingleton().shadowSceneNode[0]) {
				scene->RemoveLight(a_light.light.get());
			}
			if (a_light.light->parent) {
				a_light.light->parent->DetachChild(a_light.light.get());
			}
			a_light.light.reset();
		}

		// a_range in game units; a_life < 0: carried (see CarryLight).
		void AddLight(const RE::NiPoint3& a_at, const Color& a_color, float a_lumens, float a_range, float a_life, std::uint32_t a_key = 0)
		{
			auto* root = Root();
			auto* scene = RE::BSShaderManager::State::GetSingleton().shadowSceneNode[0];
			if (!root || !scene) {
				return;
			}
			if (g_lights.size() >= kMaxLights) {
				DropLight(g_lights.front());
				g_lights.erase(g_lights.begin());
			}
			auto* light = RE::NiPointLight::Create();
			if (!light) {
				return;
			}
			// Its colour at its brightest channel's full; how bright it is goes in its fade.
			const float top = std::max({ a_color.r, a_color.g, a_color.b, 1e-4f });
			auto&       data = light->GetLightRuntimeData();
			data.ambient = RE::NiColor{ 0.0f, 0.0f, 0.0f };
			data.diffuse = RE::NiColor{ Gamma(a_color.r / top), Gamma(a_color.g / top), Gamma(a_color.b / top) };
			data.radius = RE::NiPoint3{ a_range, a_range, a_range };
			data.fade = LightFade(a_lumens);
			light->local.translate = a_at;
			root->AttachChild(light, true);
			RE::ShadowSceneNode::LIGHT_CREATE_PARAMS params{};
			params.dynamic = true;
			params.shadowLight = false;
			params.portalStrict = false;
			params.affectLand = true;
			params.affectWater = true;
			params.neverFades = true;
			params.fov = 1.0f;
			params.falloff = 1.0f;
			params.nearDistance = 5.0f;
			params.depthBias = 0.0f;
			params.sceneGraphIndex = 0;
			params.restrictedNode = nullptr;
			params.lensFlareData = nullptr;
			scene->AddLight(light, params);
			g_lights.push_back({ RE::NiPointer<RE::NiPointLight>(light), 0.0f, a_life, data.fade, a_key, true });
		}

		// A carried light: made the first time, moved each frame it's placed, out once it isn't.
		void CarryLight(std::uint32_t a_key, const RE::NiPoint3& a_at, const Color& a_color, float a_lumens, float a_range)
		{
			for (auto& l : g_lights) {
				if (l.key == a_key && l.life < 0.0f) {
					if (l.light) {
						l.light->local.translate = a_at;
					}
					l.seen = true;
					return;
				}
			}
			AddLight(a_at, a_color, a_lumens, a_range, -1.0f, a_key);
		}

		void UpdateLights(float a_dt)
		{
			for (std::size_t i = 0; i < g_lights.size();) {
				auto&      l = g_lights[i];
				const bool over = l.life >= 0.0f ? l.age + a_dt >= l.life : !l.seen;
				l.age += a_dt;
				if (over || !l.light) {
					DropLight(l);
					g_lights.erase(g_lights.begin() + static_cast<std::ptrdiff_t>(i));
					continue;
				}
				if (l.life > 0.0f) {
					const float t = l.age / l.life;
					l.light->GetLightRuntimeData().fade = l.fade * (t < 0.1f ? 1.0f : std::max(0.0f, 1.0f - (t - 0.1f) / 0.9f));
				}
				l.seen = false;
				++i;
			}
		}

		// Bombs' lights are keyed apart from bolts'.
		constexpr std::uint32_t kBombKey = 0x80000000u;

		void UpdateBolts(float a_dt, const View& a_view)
		{
			const float m = M();
			const float width = 0.14f * m;
			for (std::size_t i = 0; i < g_bolts.size();) {
				auto& b = g_bolts[i];
				if (b.ended) {
					b.gone += a_dt;
				}
				const float k = 1.0f - b.gone / kStreakCatchUp;
				if (k <= 0.0f) {
					for (Part* part : { &b.tracer, &b.shape, &b.streak, &b.glow }) {
						Release(*part);
					}
					b = std::move(g_bolts.back());
					g_bolts.pop_back();
					continue;
				}
				if (b.ended) {
					// The bolt's gone; its streak catches up (its light goes out: no longer carried).
					Release(b.tracer);
					Release(b.shape);
				} else {
					const float s = Settings::Get().scale;
					CarryLight(b.id, b.head, g_c.boltGlow, 25000.0f * s * s, 2.5f * m);
					const RE::NiMatrix3 turn = ZTo(b.dir);
					Place(b.tracer, b.head, turn, RE::NiPoint3{ 1.2f, 1.2f, 1.8f } * m);
					Place(b.shape, b.head, turn, RE::NiPoint3{ 0.12f, 0.12f, 0.4f } * m);
				}
				Place(b.glow, b.head, a_view.facing, Splat(width * 5.0f * k));
				const RE::NiPoint3 to = b.head - b.origin;
				const float        dist = to.Length();
				const float        len = std::min(dist, 2.0f * m) * k;
				if (len < 1e-2f) {
					Place(b.streak, b.head, a_view.facing, {});
				} else {
					const RE::NiPoint3 axis = to / dist;
					const RE::NiPoint3 middle = b.head - axis * (len * 0.5f);
					Place(b.streak, middle, AxisFacing(axis, a_view.at - middle), { width, len, 1.0f });
				}
				++i;
			}
		}

		void UpdatePieces(float a_dt, const View& a_view)
		{
			for (std::size_t i = 0; i < g_pieces.size();) {
				auto& p = g_pieces[i];
				p.age += a_dt;
				if (p.age >= p.life) {
					for (auto& part : p.parts) {
						Release(part);
					}
					p = std::move(g_pieces.back());
					g_pieces.pop_back();
					continue;
				}
				const float t = p.age / p.life;
				p.vel = p.vel * std::exp(-p.drag * a_dt) - kUp * (p.gravity * a_dt);
				p.at += p.vel * a_dt;
				if (p.floor && p.at.z < *p.floor) {
					p.at.z = *p.floor;
					if (p.vel.z < 0.0f) {
						p.vel = { p.vel.x * 0.5f, p.vel.y * 0.5f, -p.vel.z * 0.35f };
						p.spin = p.spin * 0.5f;
					}
				}
				const float  grow = 1.0f - (1.0f - t) * (1.0f - t);
				RE::NiPoint3 size = p.size0 + (p.size1 - p.size0) * grow;
				if (p.shrink && t > 0.85f) {
					size = size * ((1.0f - t) / 0.15f);
				}
				RE::NiMatrix3 turn;
				switch (p.facing) {
				case Facing::kCamera:
					turn = a_view.facing * AboutZ(p.roll);
					break;
				case Facing::kVelocity:
					{
						const float l = p.vel.Length();
						turn = AxisFacing(l > 1e-3f ? p.vel / l : kUp, a_view.at - p.at);
						break;
					}
				case Facing::kAxis:
					turn = AxisFacing(p.axis, a_view.at - p.at);
					break;
				default:
					if (p.spin.x != 0.0f || p.spin.y != 0.0f || p.spin.z != 0.0f) {
						p.turn = AxisAngle(p.spin * a_dt) * p.turn;
					}
					turn = p.turn;
					break;
				}
				float a = t < p.fadeFrom ? p.alpha0 : p.alpha0 + (p.alpha1 - p.alpha0) * ((t - p.fadeFrom) / std::max(1.0f - p.fadeFrom, 1e-4f));
				a = std::clamp(a, 0.0f, 1.0f);
				for (auto& part : p.parts) {
					Place(part, p.at, turn, size);
					Tint(part, p.colorEnd ? part.color.Mix(*p.colorEnd, t) : part.color, a);
				}
				++i;
			}
		}
	}

	bool Available()
	{
		if (g_available < 0) {
			g_available = Load(g_templates["quad_glow_add"], "quad_glow_add") ? 1 : 0;
			if (g_available) {
				LoadPalette();
			}
			SKSE::log::info("fx: Rift Apart's effect art is {}", g_available ? "installed" : "missing (skse\\tools\\ratchet_vfx_to_skyrim.py): Skyrim's own looks");
		}
		return g_available == 1;
	}

	void Update(float a_dt)
	{
		if (!g_arcDrawn) {
			HideArc(0);
		}
		const bool arc = g_arcDrawn;
		g_arcDrawn = false;
		if (g_pieces.empty() && g_bolts.empty() && !arc && g_lights.empty()) {
			return;
		}
		if (!Root()) {
			return;
		}
		const View view = CameraView();
		UpdatePieces(a_dt, view);
		UpdateBolts(a_dt, view);
		UpdateLights(a_dt);
		if (g_root) {
			RE::NiUpdateData update{ 0.0f, RE::NiUpdateData::Flag::kNone };
			g_root->Update(update);
		}
	}

	void Clear()
	{
		for (auto& p : g_pieces) {
			for (auto& part : p.parts) {
				Release(part);
			}
		}
		g_pieces.clear();
		for (auto& b : g_bolts) {
			for (Part* part : { &b.tracer, &b.shape, &b.streak, &b.glow }) {
				Release(*part);
			}
		}
		g_bolts.clear();
		for (auto& part : g_arc) {
			Release(part);
		}
		g_arc.clear();
		for (auto& l : g_lights) {
			DropLight(l);
		}
		g_lights.clear();
	}

	void BombGlow(std::uint32_t a_id, const RE::NiPoint3& a_at)
	{
		if (!Available()) {
			return;
		}
		const float s = Settings::Get().scale;
		CarryLight(kBombKey | a_id, a_at, g_c.grenadeGlow, 20000.0f * s * s, 2.5f * M());
	}

	void ThrowArc(std::span<const RE::NiPoint3> a_points, std::optional<RE::NiPoint3> a_landing, float a_radius)
	{
		if (!Available() || !Root()) {
			return;
		}
		// The sandbox's: thick bright lines drawn over the world, 5 pixels wide whatever their
		// distance (here, quads as wide as the camera sees 6 mm a metre away: about 10 pixels at
		// 2560 across).
		const Color        color{ 0.35f, 0.8f, 1.0f };
		constexpr float    kAngularWidth = 0.006f;
		const View         view = CameraView();
		std::size_t        used = 0;
		const RE::NiPoint3 up{ 0.0f, 0.0f, 1.0f };
		const auto line = [&](const RE::NiPoint3& a_from, const RE::NiPoint3& a_to, bool a_flat) {
			const RE::NiPoint3 d = a_to - a_from;
			const float        len = d.Length();
			if (len < 1e-2f) {
				return;
			}
			if (used == g_arc.size()) {
				g_arc.push_back(Make("quad_white_blend", false, color));
			}
			Part&              part = g_arc[used++];
			const RE::NiPoint3 axis = d / len;
			const RE::NiPoint3 middle = (a_from + a_to) * 0.5f;
			const float        width = kAngularWidth * middle.GetDistance(view.at);
			// Along the flight its face turned to the camera; the ring's lie on the ground.
			const RE::NiMatrix3 turn = a_flat ? Columns(axis.Cross(up), axis, up) : AxisFacing(axis, view.at - middle);
			Place(part, middle, turn, { width, len, 1.0f });
			Tint(part, color, 1.0f);
		};
		// Dashed: every other step of the flight.
		for (std::size_t i = 1; i < a_points.size(); i += 2) {
			line(a_points[i - 1], a_points[i], false);
		}
		if (a_landing) {
			constexpr int      kSides = 32;
			const RE::NiPoint3 centre = *a_landing + up * (0.02f * M());
			for (int k = 0; k < kSides; ++k) {
				const float a0 = static_cast<float>(k) / kSides * kTau, a1 = static_cast<float>(k + 1) / kSides * kTau;
				line(centre + RE::NiPoint3{ std::cos(a0), std::sin(a0), 0.0f } * a_radius,
					centre + RE::NiPoint3{ std::cos(a1), std::sin(a1), 0.0f } * a_radius, true);
			}
		}
		HideArc(used);
		g_arcDrawn = true;
	}

	void MuzzleFlash(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir)
	{
		if (!Available()) {
			return;
		}
		const float m = M();
		const Color color = g_c.boltGlow * 6.0f;
		const float size = 0.1f * m;
		Piece       flash = Life(0.05f);
		flash.parts.push_back(Make("muzzle_flash_mesh_0", false, color));
		flash.at = a_at;
		flash.turn = ZTo(a_dir) * AboutZ(g_rng.Next() * kTau);
		flash.size0 = Splat(size);
		flash.size1 = Splat(size * 1.6f);
		flash.fadeFrom = 0.3f;
		Spawn(std::move(flash));
		Piece shape = Life(0.05f);
		shape.roll = g_rng.Next() * kTau;
		shape.fadeFrom = 0.4f;
		Sprite("muzzle_flash", true, a_at + a_dir * (0.06f * m), { 0.3f * m, 0.45f * m }, g_c.boltGlow * 4.0f, std::move(shape));
		Piece glow = Life(0.1f);
		glow.fadeFrom = 0.2f;
		Sprite("glow", true, a_at, { 0.6f * m, 0.9f * m }, g_c.boltGlow * 1.5f, std::move(glow));
		const float s = Settings::Get().scale;
		AddLight(a_at + a_dir * (0.1f * m), g_c.boltGlow, 80000.0f * s * s, 3.0f * m, 0.07f);
	}

	std::uint32_t Bolt(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir)
	{
		if (!Available()) {
			return 0;
		}
		BoltLook b;
		b.id = g_nextBolt++;
		b.origin = b.head = a_at;
		b.dir = a_dir;
		b.tracer = Make("tracer_mesh_0", false, g_c.bolt);
		b.shape = Make("projectile_shape_0", true, g_c.boltGlow * 1.5f);
		b.streak = Make("quad_spark_add", true, g_c.boltGlow * 6.0f);
		b.glow = Make("quad_glow_add", true, g_c.boltGlow * 4.0f);
		for (Part* part : { &b.tracer, &b.shape, &b.streak, &b.glow }) {
			Tint(*part, part->color, 1.0f);
		}
		g_bolts.push_back(std::move(b));
		return g_bolts.back().id;
	}

	void MoveBolt(std::uint32_t a_bolt, const RE::NiPoint3& a_at)
	{
		for (auto& b : g_bolts) {
			if (b.id == a_bolt && !b.ended) {
				b.head = a_at;
			}
		}
	}

	void EndBolt(std::uint32_t a_bolt)
	{
		for (auto& b : g_bolts) {
			if (b.id == a_bolt) {
				b.ended = true;
			}
		}
	}

	void BoltTrail(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
	{
		if (!Available()) {
			return;
		}
		const RE::NiPoint3 d = a_to - a_from;
		const float        len = d.Length();
		if (len < 1e-2f) {
			return;
		}
		const float m = M();
		const Color color = g_c.boltGlow * 1.5f;
		Piece       trail = Life(0.35f);
		trail.parts.push_back(Make("quad_smoke_trail_add", true, color));
		trail.at = (a_from + a_to) * 0.5f;
		trail.size0 = { 0.06f * m, len, 1.0f };
		trail.size1 = { 0.22f * m, len, 1.0f };
		trail.colorEnd = g_c.impactGlowEnd;
		trail.facing = Facing::kAxis;
		trail.axis = d / len;
		Spawn(std::move(trail));
	}

	void BoltImpact(const RE::NiPoint3& a_at, const RE::NiPoint3& a_normal, bool a_mark)
	{
		if (!Available()) {
			return;
		}
		const float        m = M();
		const RE::NiPoint3 off = a_at + a_normal * (0.05f * m);
		Piece              flare = Life(0.1f);
		flare.roll = g_rng.Next() * kTau;
		flare.fadeFrom = 0.1f;
		Sprite("impact_flare", false, off, { 0.4f * m, 1.1f * m }, g_c.impactFlash * 6.0f, std::move(flare));
		Piece glow = Life(0.2f);
		glow.colorEnd = g_c.impactGlowEnd;
		Sprite("glow", true, off, { 0.6f * m, 1.0f * m }, g_c.impactGlow * 2.0f, std::move(glow));
		const float s = Settings::Get().scale;
		AddLight(a_at + a_normal * (0.3f * m), g_c.impactGlow, 40000.0f * s * s, 2.5f * m, 0.12f);
		const std::optional<float> floor = a_normal.z > 0.5f ? std::optional(a_at.z) : std::nullopt;
		for (int i = 0; i < 9; ++i) {
			RE::NiPoint3 dir = a_normal + g_rng.Unit() * 0.9f;
			const float  l = dir.Length();
			dir = l > 1e-4f ? dir / l : a_normal;
			const float w = 0.025f * m;
			Piece       spark = Life(0.25f + 0.2f * g_rng.Next());
			spark.parts.push_back(Make("quad_spark_blend", false, g_c.impactSpark * 5.0f));
			spark.at = off;
			spark.vel = dir * ((3.0f + 5.0f * g_rng.Next()) * m);
			spark.gravity = 9.0f * m;
			spark.drag = 1.5f;
			spark.size0 = { w, w * 6.0f, 1.0f };
			spark.size1 = { w * 0.5f, w * 3.0f, 1.0f };
			spark.fadeFrom = 0.4f;
			spark.colorEnd = g_c.impactGlowEnd * 3.0f;
			spark.facing = Facing::kVelocity;
			spark.floor = floor;
			Spawn(std::move(spark));
		}
		Piece smoke = Life(0.7f);
		smoke.vel = (a_normal * 0.4f + kUp * 0.6f) * m;
		smoke.drag = 0.5f;
		smoke.alpha0 = 0.4f;
		smoke.fadeFrom = 0.2f;
		smoke.roll = g_rng.Next() * kTau;
		Sprite("smoke_puff", false, off, { 0.25f * m, 0.8f * m }, g_c.impactSmoke * 2.0f, std::move(smoke));
		if (a_mark) {
			const RE::NiMatrix3 turn = ZTo(a_normal) * AboutZ(g_rng.Next() * kTau);
			const float         size = 0.22f * m;
			Piece               mark = Life(8.0f);
			mark.parts.push_back(Make("quad_bullet_decal_blend", false, { 0.3f, 0.32f, 0.3f }));
			mark.at = a_at + a_normal * kMarkLift;
			mark.turn = turn;
			mark.size0 = mark.size1 = Splat(size);
			mark.alpha0 = 0.9f;
			mark.fadeFrom = 0.8f;
			Spawn(std::move(mark));
			Piece hot = Life(0.8f);
			hot.parts.push_back(Make("quad_glow_simple_add", true, g_c.decalGlow * 4.0f));
			hot.at = a_at + a_normal * (kMarkLift * 1.5f);
			hot.turn = turn;
			hot.size0 = Splat(size * 0.8f);
			hot.size1 = Splat(size * 0.6f);
			hot.fadeFrom = 0.1f;
			Spawn(std::move(hot));
		}
	}

	void Shell(const RE::NiPoint3& a_at, const RE::NiMatrix3& a_turn, std::optional<float> a_floor)
	{
		if (!Available()) {
			return;
		}
		// wpn_burstpistol_weapon.json ShellCasingData: thrown out at 2-4 m/s, tumbling up to
		// (45, 5, 45) about the ejector's axes (the sandbox reads them as radians/s).
		const float        m = M();
		const float        speed = (2.0f + 2.0f * g_rng.Next()) * m;
		const RE::NiPoint3 out = a_turn * RE::NiPoint3{ 0.0f, 0.0f, 1.0f };
		Piece              shell = Life(1.6f + 0.4f * g_rng.Next());
		shell.parts.push_back(Make("shell_0", false, g_c.shellGlass * 5.0f, 0.45f));
		shell.parts.push_back(Make("shell_1", true, g_c.boltGlow));
		shell.parts.push_back(Make("shell_2", false, { 0.62f, 0.63f, 0.66f }));
		shell.at = a_at;
		shell.vel = out * speed + g_rng.Unit() * (0.3f * m);
		shell.gravity = 9.8f * m;
		shell.spin = a_turn * RE::NiPoint3{ 45.0f * g_rng.Signed(), 5.0f * g_rng.Signed(), 45.0f * g_rng.Signed() };
		shell.turn = a_turn;
		shell.size0 = shell.size1 = Splat(m);
		shell.alpha0 = shell.alpha1 = 1.0f;
		if (a_floor) {
			shell.floor = *a_floor + 0.02f * m;
		}
		shell.shrink = true;
		Spawn(std::move(shell));
	}

	void Explosion(const RE::NiPoint3& a_at, float a_radius, std::optional<float> a_ground)
	{
		if (!Available()) {
			return;
		}
		const float m = M();
		const float s = Settings::Get().scale;
		AddLight(a_at + kUp * (0.6f * m), g_c.blastFire, 3.0e6f * s * s, a_radius * 3.0f, 0.5f);
		Piece       flash = Life(0.16f);
		flash.roll = g_rng.Next() * kTau;
		flash.fadeFrom = 0.1f;
		Sprite("flare_radial", true, a_at, { 2.5f * m, 7.0f * m }, g_c.blastHot * 6.0f, std::move(flash));
		// The fireball: a hot core and puffs of the game's smoke burning orange to deep orange.
		Piece core = Life(0.3f);
		core.fadeFrom = 0.2f;
		Sprite("glow", true, a_at, { 1.5f * m, 3.5f * m }, g_c.blastFire * 4.0f, std::move(core));
		for (int i = 0; i < 8; ++i) {
			RE::NiPoint3 dir{ g_rng.Signed(), g_rng.Signed(), 0.3f + g_rng.Next() };
			dir.Unitize();
			Piece fire = Life(0.3f + 0.2f * g_rng.Next());
			fire.vel = dir * ((3.0f + 3.0f * g_rng.Next()) * m);
			fire.drag = 3.5f;
			fire.alpha0 = 0.95f;
			fire.colorEnd = g_c.blastFireEnd * 2.5f;
			fire.fadeFrom = 0.35f;
			fire.roll = g_rng.Next() * kTau;
			Sprite("smoke_puff", false, a_at + dir * (0.3f * m), { 1.1f * m, 2.8f * m }, g_c.blastFire * 8.0f, std::move(fire));
		}
		// The game's shockwave dome with its noise mask, in the explosion's violet (its z is up).
		const float r = a_radius * 0.45f;
		Piece       dome = Life(0.25f);
		dome.parts.push_back(Make("shockwave_hemisphere_0_blend", false, g_c.blastEnergy * 2.0f));
		dome.at = a_at;
		dome.size0 = Splat(r * 0.2f);
		dome.size1 = Splat(r);
		dome.alpha0 = 0.7f;
		dome.fadeFrom = 0.1f;
		Spawn(std::move(dome));
		Piece energy = Life(0.35f);
		energy.fadeFrom = 0.1f;
		Sprite("glow_simple", true, a_at, { a_radius * 0.4f, a_radius * 1.2f }, g_c.blastEnergy * 2.5f, std::move(energy));
		// The grenade shatters: its crystal shards fly out, spinning, glowing violet.
		const std::optional<float> floor = a_ground ? std::optional(*a_ground + 0.03f * m) : std::nullopt;
		for (int i = 0; i < 12; ++i) {
			RE::NiPoint3 dir{ g_rng.Signed(), g_rng.Signed(), 0.5f + g_rng.Next() };
			dir.Unitize();
			Piece shard = Life(0.9f + 0.4f * g_rng.Next());
			shard.parts.push_back(Make(std::format("shatter_shards_{}", i % 5), false, g_c.blastShards * 2.0f));
			shard.at = a_at;
			shard.vel = dir * ((6.0f + 6.0f * g_rng.Next()) * m);
			shard.gravity = 14.0f * m;
			shard.drag = 0.4f;
			shard.spin = g_rng.Unit() * 12.0f;
			shard.turn = AxisAngle(g_rng.Unit() * 3.0f);
			shard.size0 = shard.size1 = Splat((0.6f + 0.4f * g_rng.Next()) * m);
			shard.alpha0 = shard.alpha1 = 0.9f;
			shard.floor = floor;
			shard.shrink = true;
			Spawn(std::move(shard));
		}
		// Smoke, rising and spreading slowly.
		for (int i = 0; i < 6; ++i) {
			RE::NiPoint3 dir{ g_rng.Signed(), g_rng.Signed(), 0.0f };
			const float  l = dir.Length();
			dir = l > 1e-4f ? dir / l : RE::NiPoint3{ 1.0f, 0.0f, 0.0f };
			Piece smoke = Life(1.4f + 0.8f * g_rng.Next());
			smoke.vel = (dir * 0.8f + kUp * (0.5f + 0.5f * g_rng.Next())) * m;
			smoke.drag = 0.8f;
			smoke.gravity = -0.3f * m;
			smoke.alpha0 = 0.45f;
			smoke.fadeFrom = 0.2f;
			smoke.roll = g_rng.Next() * kTau;
			Sprite("smoke_puff", false, a_at + dir * (0.4f * m) + kUp * (0.3f * m), { 1.2f * m, 3.2f * m }, g_c.blastSmoke * 3.0f, std::move(smoke));
		}
		// Debris.
		for (int i = 0; i < 10; ++i) {
			RE::NiPoint3 dir{ g_rng.Signed(), g_rng.Signed(), 0.8f + g_rng.Next() };
			dir.Unitize();
			Piece chunk = Life(0.8f + 0.4f * g_rng.Next());
			chunk.vel = dir * ((5.0f + 5.0f * g_rng.Next()) * m);
			chunk.gravity = 16.0f * m;
			chunk.floor = floor;
			chunk.fadeFrom = 0.7f;
			chunk.roll = g_rng.Next() * kTau;
			Sprite("debris_chunk", false, a_at, { 0.12f * m, 0.12f * m }, { 0.45f, 0.42f, 0.4f }, std::move(chunk));
		}
		// A scorch mark, with violet embers cooling on it.
		if (a_ground) {
			const RE::NiMatrix3 flat = AboutZ(g_rng.Next() * kTau);
			const float         size = a_radius * 0.9f;
			const RE::NiPoint3  on{ a_at.x, a_at.y, *a_ground };
			Piece               scorch = Life(10.0f);
			scorch.parts.push_back(Make("quad_scorch_mask_blend", false, { 0.03f, 0.025f, 0.03f }));
			scorch.at = on + kUp * kMarkLift;
			scorch.turn = flat;
			scorch.size0 = scorch.size1 = Splat(size);
			scorch.alpha0 = 0.8f;
			scorch.fadeFrom = 0.85f;
			Spawn(std::move(scorch));
			Piece embers = Life(2.5f);
			embers.parts.push_back(Make("quad_scorch_glow_add", true, g_c.scorchGlow * 6.0f));
			embers.at = on + kUp * (kMarkLift * 1.5f);
			embers.turn = flat;
			embers.size0 = embers.size1 = Splat(size);
			embers.fadeFrom = 0.15f;
			Spawn(std::move(embers));
			Piece glow = Life(0.9f);
			glow.parts.push_back(Make("quad_glow_simple_add", true, g_c.scorchGlow * 1.5f));
			glow.at = on + kUp * (kMarkLift * 2.0f);
			glow.turn = flat;
			glow.size0 = Splat(size * 0.5f);
			glow.size1 = Splat(size * 0.8f);
			Spawn(std::move(glow));
		}
	}

	void WrenchHit(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir, std::optional<float> a_floor)
	{
		if (!Available()) {
			return;
		}
		const float m = M();
		Piece       flash = Life(0.12f);
		flash.roll = g_rng.Next() * kTau;
		flash.fadeFrom = 0.1f;
		Sprite("flare_radial", false, a_at, { 0.5f * m, 1.4f * m }, g_c.wrenchHit * 4.0f, std::move(flash));
		const float s = Settings::Get().scale;
		AddLight(a_at - a_dir * (0.3f * m), g_c.wrenchHit, 60000.0f * s * s, 3.0f * m, 0.12f);
		// An anamorphic streak across the flash.
		Piece streak = Life(0.15f);
		streak.parts.push_back(Make("quad_glow_add", true, g_c.wrenchHit * 3.0f));
		streak.at = a_at;
		streak.size0 = { 0.6f * m, 0.08f * m, 1.0f };
		streak.size1 = { 1.8f * m, 0.03f * m, 1.0f };
		streak.facing = Facing::kCamera;
		Spawn(std::move(streak));
		for (int i = 0; i < 10; ++i) {
			RE::NiPoint3 v = a_dir * -1.0f + g_rng.Unit() * 0.9f + kUp * 0.3f;
			const float  l = v.Length();
			v = l > 1e-4f ? v / l : kUp;
			const float w = 0.02f * m;
			Piece       spark = Life(0.25f + 0.15f * g_rng.Next());
			spark.parts.push_back(Make("quad_spark_blend", false, g_c.wrenchHit * 5.0f));
			spark.at = a_at;
			spark.vel = v * ((3.0f + 5.0f * g_rng.Next()) * m);
			spark.gravity = 9.0f * m;
			spark.drag = 1.5f;
			spark.size0 = { w, w * 7.0f, 1.0f };
			spark.size1 = { w * 0.5f, w * 3.0f, 1.0f };
			spark.fadeFrom = 0.4f;
			spark.colorEnd = g_c.wrenchTrailEnd * 3.0f;
			spark.facing = Facing::kVelocity;
			spark.floor = a_floor;
			Spawn(std::move(spark));
		}
		Piece smoke = Life(0.6f);
		smoke.vel = (kUp * 0.6f - a_dir * 0.3f) * m;
		smoke.drag = 0.5f;
		smoke.alpha0 = 0.35f;
		smoke.fadeFrom = 0.2f;
		smoke.roll = g_rng.Next() * kTau;
		Sprite("smoke_puff", false, a_at, { 0.3f * m, 0.8f * m }, g_c.wrenchSmoke * 2.0f, std::move(smoke));
	}

	void WrenchSlam(const RE::NiPoint3& a_at, float a_radius)
	{
		if (!Available()) {
			return;
		}
		const float        m = M();
		const RE::NiPoint3 up = a_at + kUp * (0.2f * m);
		const float        s = Settings::Get().scale;
		AddLight(a_at + kUp * (0.8f * m), g_c.wrenchSlam, 1.2e6f * s * s, a_radius * 3.0f, 0.4f);
		Piece              flash = Life(0.15f);
		flash.roll = g_rng.Next() * kTau;
		flash.fadeFrom = 0.1f;
		Sprite("flare_radial", true, up, { 1.5f * m, 4.0f * m }, g_c.wrenchSlam * 6.0f, std::move(flash));
		Piece glow = Life(0.35f);
		glow.fadeFrom = 0.1f;
		Sprite("glow_simple", true, up, { a_radius * 0.5f, a_radius * 1.2f }, g_c.wrenchSlamGlow * 2.5f, std::move(glow));
		// The game's impact-blast shapes, bursting up (they're y-up meshes, turned about up).
		for (int i = 0; i < 4; ++i) {
			const float size = 1.4f * m;
			const float a = g_rng.Next() * kTau;
			const float c = std::cos(a), sn = std::sin(a);
			Piece       blast = Life(0.3f);
			blast.parts.push_back(Make(std::format("impact_blast_{}", i), true, g_c.wrenchSlam * 2.0f));
			blast.at = a_at;
			blast.turn = YUp(Columns({ c, 0.0f, -sn }, { 0.0f, 1.0f, 0.0f }, { sn, 0.0f, c }));
			blast.size0 = Splat(size * 0.3f);
			blast.size1 = Splat(size);
			blast.fadeFrom = 0.2f;
			Spawn(std::move(blast));
		}
		// Blast masks around the ring, rising (the texture's burst points down: turned over).
		for (int k = 0; k < 6; ++k) {
			const float  a = static_cast<float>(k) / 6.0f * kTau + g_rng.Next() * 0.4f;
			const RE::NiPoint3 p = a_at + RE::NiPoint3{ std::cos(a), std::sin(a), 0.0f } * (0.6f * m) + kUp * (0.4f * m);
			Piece        mask = Life(0.3f);
			mask.vel = kUp * (1.5f * m);
			mask.drag = 2.0f;
			mask.roll = kPi + g_rng.Signed() * 0.3f;
			mask.fadeFrom = 0.3f;
			Sprite("blast_masks", false, p, { 0.6f * m, 1.6f * m }, g_c.wrenchSlam * 2.0f, std::move(mask));
		}
		// A ring along the ground: the game's shockwave dome, flattened (its z is up).
		Piece ring = Life(0.35f);
		ring.parts.push_back(Make("shockwave_hemisphere_0_add", true, g_c.wrenchSlam * 2.0f));
		ring.at = a_at;
		ring.size0 = { a_radius * 0.2f, a_radius * 0.2f, 0.05f * m };
		ring.size1 = { a_radius, a_radius, 0.3f * m };
		ring.fadeFrom = 0.2f;
		Spawn(std::move(ring));
		// Dust rolling out along the ground, and debris.
		for (int k = 0; k < 8; ++k) {
			const float        a = static_cast<float>(k) / 8.0f * kTau + g_rng.Next() * 0.5f;
			const RE::NiPoint3 out{ std::cos(a), std::sin(a), 0.0f };
			Piece              dust = Life(0.9f + 0.4f * g_rng.Next());
			dust.vel = (out * (2.5f + 1.5f * g_rng.Next()) + kUp * 0.4f) * m;
			dust.drag = 2.0f;
			dust.alpha0 = 0.5f;
			dust.fadeFrom = 0.25f;
			dust.roll = g_rng.Next() * kTau;
			Sprite("smoke_puff", false, a_at + out * (0.4f * m) + kUp * (0.2f * m), { 0.6f * m, 1.6f * m }, g_c.wrenchSmoke * 3.0f, std::move(dust));
		}
		for (int i = 0; i < 10; ++i) {
			RE::NiPoint3 dir{ g_rng.Signed(), g_rng.Signed(), 1.0f + g_rng.Next() };
			dir.Unitize();
			Piece chunk = Life(0.8f + 0.4f * g_rng.Next());
			chunk.vel = dir * ((4.0f + 4.0f * g_rng.Next()) * m);
			chunk.gravity = 16.0f * m;
			chunk.floor = a_at.z + 0.03f * m;
			chunk.fadeFrom = 0.7f;
			chunk.roll = g_rng.Next() * kTau;
			Sprite("debris_chunk", false, up, { 0.1f * m, 0.1f * m }, { 0.45f, 0.42f, 0.4f }, std::move(chunk));
		}
		// The crater, and blue embers cooling in it.
		const RE::NiMatrix3 flat = AboutZ(g_rng.Next() * kTau);
		const float         size = a_radius * 0.7f;
		Piece               crater = Life(10.0f);
		crater.parts.push_back(Make("quad_crater_blend", false, { 0.6f, 0.6f, 0.6f }));
		crater.at = a_at + kUp * kMarkLift;
		crater.turn = flat;
		crater.size0 = crater.size1 = Splat(size);
		crater.alpha0 = 0.9f;
		crater.fadeFrom = 0.85f;
		Spawn(std::move(crater));
		Piece embers = Life(1.5f);
		embers.parts.push_back(Make("quad_scorch_glow_add", true, g_c.slamScorch * 5.0f));
		embers.at = a_at + kUp * (kMarkLift * 1.5f);
		embers.turn = flat;
		embers.size0 = embers.size1 = Splat(size);
		embers.fadeFrom = 0.15f;
		Spawn(std::move(embers));
	}

	void WrenchTrail(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
	{
		if (!Available()) {
			return;
		}
		const RE::NiPoint3 d = a_to - a_from;
		const float        len = d.Length();
		if (len < 1e-2f) {
			return;
		}
		const float m = M();
		Piece       trail = Life(0.4f);
		trail.parts.push_back(Make("quad_trail_basic_blend", false, g_c.wrenchTrail * 3.0f));
		trail.at = (a_from + a_to) * 0.5f;
		trail.size0 = { 0.5f * m, len, 1.0f };
		trail.size1 = { 0.15f * m, len, 1.0f };
		trail.alpha0 = 0.95f;
		trail.colorEnd = g_c.wrenchTrailEnd * 2.0f;
		trail.facing = Facing::kAxis;
		trail.axis = d / len;
		Spawn(std::move(trail));
	}
}
