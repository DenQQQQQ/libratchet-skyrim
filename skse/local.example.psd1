# This PC's paths for build.ps1 and test\ingame.ps1. Copy to local.psd1 (not in git) and edit;
# a parameter given on the command line wins over these.
@{
    # MO2's folder (ingame.ps1 starts the game through it).
    Mo2 = "C:\Modding\MO2"
    # The MO2 mod folder build.ps1 -Install fills and ingame.ps1 tests.
    ModDir = "C:\Modding\MO2\mods\Ratchet Movement"
    # The MO2 profile (its name, or its folder) whose plugins.txt gets RatchetMovement.esl.
    Mo2Profile = "Default"
    # RAD Video Tools' encoder, for the startup video (optional).
    RadVideo = "C:\Tools\RADTools\bin\radvideo64.exe"
    # Visual Studio or its Build Tools, when cl.exe isn't on PATH and vswhere doesn't find it.
    VsDir = ""
    # vcpkg, when VCPKG_ROOT isn't set.
    VcpkgRoot = "C:\vcpkg"
}
