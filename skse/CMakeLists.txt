# SPDX-License-Identifier: GPL-3.0-or-later
# The SKSE plugin: libratchet's movement on Skyrim's player. GPL-3.0-or-later because it links
# CommonLibSSE-NG (GPL-3.0-or-later with a modding exception); libratchet itself stays MIT.
cmake_minimum_required(VERSION 3.25)

project(RatchetMovement VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")

# The repo checkout this folder sits in: libratchet-ffi is built from its crates/.
cmake_path(GET CMAKE_CURRENT_SOURCE_DIR PARENT_PATH RATCHET_CHECKOUT)
set(RATCHET_CARGO_TARGET_DIR "${RATCHET_CHECKOUT}/target/skse" CACHE PATH "CARGO_TARGET_DIR for libratchet-ffi")

# --- libratchet-ffi (ratchet.dll), built by cargo every build (cargo skips it when nothing changed)
find_program(CARGO cargo REQUIRED)
set(RATCHET_DLL "${RATCHET_CARGO_TARGET_DIR}/release/ratchet.dll")
set(RATCHET_IMPLIB "${RATCHET_CARGO_TARGET_DIR}/release/ratchet.dll.lib")
add_custom_target(
	ratchet_ffi
	COMMAND "${CMAKE_COMMAND}" -E env "CARGO_TARGET_DIR=${RATCHET_CARGO_TARGET_DIR}"
		"${CARGO}" build --release -p libratchet-ffi
	WORKING_DIRECTORY "${RATCHET_CHECKOUT}"
	BYPRODUCTS "${RATCHET_DLL}" "${RATCHET_IMPLIB}"
	USES_TERMINAL
	COMMENT "cargo build -p libratchet-ffi (release)"
)

# --- CommonLibSSE-NG, pinned. AE only (1.6.x, both struct layouts): the owner runs 1.6.1170.
set(ENABLE_SKYRIM_SE OFF CACHE BOOL "" FORCE)
set(ENABLE_SKYRIM_AE ON CACHE BOOL "" FORCE)
set(ENABLE_SKYRIM_VR OFF CACHE BOOL "" FORCE)
set(BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(COMMONLIB_ENABLE_IPO OFF CACHE BOOL "" FORCE)
include(FetchContent)
FetchContent_Declare(
	CommonLibSSE
	GIT_REPOSITORY https://github.com/alandtse/CommonLibSSE-NG.git
	GIT_TAG v11.0.0
	GIT_SHALLOW TRUE
	GIT_SUBMODULES ""
	EXCLUDE_FROM_ALL
)
FetchContent_MakeAvailable(CommonLibSSE)

# --- the plugin
file(GLOB_RECURSE PLUGIN_SOURCES CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/src/*.h")
add_library(${PROJECT_NAME} SHARED ${PLUGIN_SOURCES})
add_dependencies(${PROJECT_NAME} ratchet_ffi)
target_include_directories(${PROJECT_NAME} PRIVATE src "${RATCHET_CHECKOUT}/crates/libratchet-ffi/include")
target_compile_definitions(${PROJECT_NAME} PRIVATE
	PLUGIN_NAME="${PROJECT_NAME}"
	PLUGIN_VERSION_MAJOR=${PROJECT_VERSION_MAJOR}
	PLUGIN_VERSION_MINOR=${PROJECT_VERSION_MINOR}
	PLUGIN_VERSION_PATCH=${PROJECT_VERSION_PATCH})
target_compile_options(${PROJECT_NAME} PRIVATE /W4 /permissive- /utf-8 /Zc:__cplusplus /external:anglebrackets /external:W0)
target_precompile_headers(${PROJECT_NAME} PRIVATE src/PCH.h)
# ratchet.dll is delay-loaded: SKSE loads the plugin by its path, which doesn't put the plugin's
# folder on the DLL search path, so the plugin loads ratchet.dll from its own folder first.
target_link_libraries(${PROJECT_NAME} PRIVATE CommonLibSSE::CommonLibSSE "${RATCHET_IMPLIB}" delayimp.lib)
target_link_options(${PROJECT_NAME} PRIVATE /DELAYLOAD:ratchet.dll /DEBUG:FULL /INCREMENTAL:NO /OPT:REF /OPT:ICF)

# Stage the mod's files the way MO2 lays them out (SKSE/Plugins/...), next to the build.
# ratchet.dll goes in the plugin's own subfolder: SKSE tries every DLL in Plugins as a plugin.
set(STAGE_DIR "${CMAKE_BINARY_DIR}/stage/SKSE/Plugins")
add_custom_command(
	TARGET ${PROJECT_NAME} POST_BUILD
	COMMAND "${CMAKE_COMMAND}" -E make_directory "${STAGE_DIR}/RatchetMovement"
	COMMAND "${CMAKE_COMMAND}" -E copy_if_different "$<TARGET_FILE:${PROJECT_NAME}>" "$<TARGET_PDB_FILE:${PROJECT_NAME}>"
		"${CMAKE_CURRENT_SOURCE_DIR}/RatchetMovement.ini" "${STAGE_DIR}"
	COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${RATCHET_DLL}" "${STAGE_DIR}/RatchetMovement"
	VERBATIM
)
