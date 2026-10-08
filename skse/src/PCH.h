// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Some of CommonLib's dependencies include Windows.h themselves: no min/max macros from it.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX

#include <RE/Skyrim.h>
#include <SKSE/SKSE.h>

#include <Windows.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <optional>
#include <string>

#include "ratchet.h"

using namespace std::literals;
