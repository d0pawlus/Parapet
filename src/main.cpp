#include "app.h"
#include <shellapi.h>
#include <wtsapi32.h>
#include <algorithm>

#ifndef CREATE_WAITABLE_TIMER_HIGH_RESOLUTION
#define CREATE_WAITABLE_TIMER_HIGH_RESOLUTION 0x00000002
#endif

#ifdef OVERLAY_DEBUG
Profile g_profile;
double ProfileNow()
{
    LARGE_INTEGER t, f;
    QueryPerformanceCounter(&t);
    QueryPerformanceFrequency(&f);
    return double(t.QuadPart) * 1000.0 / double(f.QuadPart);
}
#endif

namespace
{
    const UINT WM_WEATHER = WM_APP + 1, WM_TRAY = WM_APP + 2;
    const wchar_t* AppName = L"Parapet 0.1.0";
    const wchar_t* RunKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const wchar_t* RunValue = L"Parapet";

    struct Choice { const wchar_t* key; const wchar_t* label; };
    const Choice Modes[] = {
        { L"Auto", L"Automatycznie (wg pogody)" }, { L"Leaves", L"Jesienne liście" }, { L"Rain", L"Deszcz" },
        { L"Snow", L"Śnieg" }, { L"Storm", L"Jesienna burza" }, { L"Off", L"Wyłączone" },
    };
    const float Intensities[] = { 0.5f, 1.0f, 1.75f, 3.0f };
    const wchar_t* IntensityLabels[] = { L"Mała", L"Średnia", L"Duża", L"Ekstremalna" };
    const int FpsChoices[] = { 20, 30, 60 };
    const wchar_t* CritterLabels[CritterKinds] = {
        L"Jeż (jesienią, zbiera liście)", L"Gąsienica (zjada leżące liście)", L"Lis polarny (wyskakuje z zasp)",
        L"Przebiśniegi (gdy się ociepla)", L"Ślimak (po deszczu)", L"Żaba (skacze po kałużach)",
        L"Rudzik (przysiada na oknach)", L"Wiewiórka (z żołędziem)", L"Grzyby (wyrastają z błota)",
    };
    // render scale per quality level; Ultra also draws soft leaf shadows (twice the leaf sprites)
    const float QualityScales[] = { 0.5f, 0.75f, 1.0f, 1.0f };
    const wchar_t* QualityLabels[] = {
        L"Jakość: niska (½ rozdzielczości, najlżejsza)", L"Jakość: średnia (¾ rozdzielczości)",
        L"Jakość: wysoka (pełna rozdzielczość)", L"Jakość: ultra (pełna + cienie liści)",
    };

    enum : UINT
    {
        IdMode = 100, IdIntensity = 120, IdFps = 140, IdQuality = 160,
        IdIgnoreMax = 200, IdPauseFullscreen, IdBattery, IdRefresh, IdEdit, IdAutostart, IdExit, IdRare,
        IdCritter = 300,   // + CritterKind, IdCritter + CritterKinds = random
    };

    struct App
    {
        HWND overlay = nullptr, tray = nullptr;
        UINT taskbarCreated = 0;
        Settings settings;
        WeatherService weather;
        WeatherState weatherNow;
        Tracker tracker;
        Sim sim;
        Renderer renderer;
        RECT screen = {};
        std::vector<RECT> monitors, workAreas;
        bool graphicsOk = false, cleared = true, trackDirty = true, moving = false, haveCursor = false;
        float cursorX = 0, cursorY = 0, cursorVX = 0, cursorVY = 0;
        int refreshHz = 60;
        bool locked = false, displayOff = false, onBattery = false, ecoQos = false;
        HANDLE timer = nullptr;
        double freq = 1, frameStart = 0, lastFrame = 0, lastTrack = 0, lastHousekeeping = 0;
        FILETIME iniTime = {};
        HICON icon = nullptr;
        std::vector<HWINEVENTHOOK> hooks;
    } app;

    double Now()
    {
        LARGE_INTEGER t;
        QueryPerformanceCounter(&t);
        return double(t.QuadPart) / app.freq;
    }

    // ------------------------------------------------------------ screen + graphics

    BOOL CALLBACK CollectMonitor(HMONITOR m, HDC, LPRECT, LPARAM)
    {
        MONITORINFO mi = { sizeof mi };
        if (GetMonitorInfoW(m, &mi))
        {
            app.monitors.push_back(mi.rcMonitor);
            app.workAreas.push_back(mi.rcWork);
        }
        return TRUE;
    }

    bool RefreshMonitors()
    {
        app.monitors.clear();
        app.workAreas.clear();
        EnumDisplayMonitors(nullptr, nullptr, CollectMonitor, 0);
        RECT vs;
        vs.left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        vs.top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        vs.right = vs.left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
        vs.bottom = vs.top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        bool changed = !EqualRect(&vs, &app.screen);
        app.screen = vs;

        std::vector<RECT> local = app.workAreas, screens = app.monitors;
        for (RECT& r : local) OffsetRect(&r, -vs.left, -vs.top);
        for (RECT& r : screens) OffsetRect(&r, -vs.left, -vs.top);
        app.sim.SetGround(local, screens);
        return changed;
    }

    float PixelScale()
    {
        return std::max(1.0f, std::min(3.0f, GetSystemMetrics(SM_CYSCREEN) / 1080.0f));
    }

    float RenderScale()
    {
        return QualityScales[std::max(0, std::min(3, app.settings.quality))];
    }

    void InitGraphics()
    {
        app.renderer.Release();
        int w = app.screen.right - app.screen.left, h = app.screen.bottom - app.screen.top;
        SetWindowPos(app.overlay, HWND_TOPMOST, app.screen.left, app.screen.top, w, h, SWP_NOACTIVATE);
        if (app.sim.W != w || app.sim.H != h)
        {
            app.sim.Reset(w, h, PixelScale());
            RefreshMonitors();
        }
        app.graphicsOk = app.renderer.Init(app.overlay, w, h, RenderScale(), app.settings.quality >= 3);
        app.cleared = true;
        // hand back pages touched only during start-up
        SetProcessWorkingSetSize(GetCurrentProcess(), SIZE_T(-1), SIZE_T(-1));
    }

    // ------------------------------------------------------------ power awareness

    void SetEcoQos(bool on)
    {
        if (on == app.ecoQos) return;
        PROCESS_POWER_THROTTLING_STATE s = {};
        s.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
        s.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
        s.StateMask = on ? PROCESS_POWER_THROTTLING_EXECUTION_SPEED : 0;
        SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &s, sizeof s);
        app.ecoQos = on;
    }

    void RefreshPower()
    {
        SYSTEM_POWER_STATUS ps;
        app.onBattery = GetSystemPowerStatus(&ps) && ps.ACLineStatus == 0;
        SetEcoQos(app.onBattery && app.settings.batterySaver);
    }

    int Fps()
    {
        int fps = app.settings.fps;
        if (app.onBattery && app.settings.batterySaver) fps = std::min(fps, 20);
        return fps;
    }

    // Frames are paced by presenting in step with the primary display: every frame is shown for the same
    // whole number of refreshes, the one that comes closest to the chosen frame rate.
    void RefreshPacing()
    {
        DEVMODEW dm = {};
        dm.dmSize = sizeof dm;
        if (EnumDisplaySettingsW(nullptr, ENUM_CURRENT_SETTINGS, &dm) && dm.dmDisplayFrequency > 1) app.refreshHz = int(dm.dmDisplayFrequency);
        app.renderer.syncInterval = UINT(std::max(1L, std::min(10L, std::lround(double(app.refreshHz) / Fps()))));
    }

    // ------------------------------------------------------------ settings + tray

    FILETIME IniTime()
    {
        WIN32_FILE_ATTRIBUTE_DATA a = {};
        GetFileAttributesExW(Settings::Path().c_str(), GetFileExInfoStandard, &a);
        return a.ftLastWriteTime;
    }

    void SaveSettings()
    {
        app.settings.Save();
        app.iniTime = IniTime();
    }

    void UpdateTooltip()
    {
        NOTIFYICONDATAW nid = { sizeof nid };
        nid.hWnd = app.tray;
        nid.uID = 1;
        nid.uFlags = NIF_TIP;
        std::wstring tip = std::wstring(AppName) + L"\n" + app.weatherNow.Describe();
        wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }

    HICON MakeIcon()
    {
        // a maple leaf, rasterised with 4x4 supersampling
        static const float leaf[][2] = {
            { 0, -0.5f }, { 0.08f, -0.28f }, { 0.22f, -0.36f }, { 0.18f, -0.12f }, { 0.42f, -0.18f }, { 0.34f, 0.0f },
            { 0.46f, 0.06f }, { 0.22f, 0.18f }, { 0.26f, 0.30f }, { 0.04f, 0.24f }, { 0.03f, 0.5f }, { -0.03f, 0.5f },
            { -0.04f, 0.24f }, { -0.26f, 0.30f }, { -0.22f, 0.18f }, { -0.46f, 0.06f }, { -0.34f, 0.0f }, { -0.42f, -0.18f },
            { -0.18f, -0.12f }, { -0.22f, -0.36f }, { -0.08f, -0.28f },
        };
        const int n = ARRAYSIZE(leaf);
        int size = GetSystemMetrics(SM_CXSMICON);
        BITMAPV5HEADER bi = {};
        bi.bV5Size = sizeof bi;
        bi.bV5Width = size;
        bi.bV5Height = -size;
        bi.bV5Planes = 1;
        bi.bV5BitCount = 32;
        bi.bV5Compression = BI_RGB;
        void* bits = nullptr;
        HDC screenDc = GetDC(nullptr);
        HBITMAP color = CreateDIBSection(screenDc, reinterpret_cast<BITMAPINFO*>(&bi), DIB_RGB_COLORS, &bits, nullptr, 0);
        ReleaseDC(nullptr, screenDc);
        uint32_t* px = static_cast<uint32_t*>(bits);
        for (int y = 0; y < size; y++)
        {
            for (int x = 0; x < size; x++)
            {
                int hits = 0;
                for (int sy = 0; sy < 4; sy++)
                {
                    for (int sx = 0; sx < 4; sx++)
                    {
                        float fx = ((x + (sx + 0.5f) / 4) / size - 0.5f) * 1.05f;
                        float fy = ((y + (sy + 0.5f) / 4) / size - 0.5f) * 1.05f;
                        bool in = false;
                        for (int i = 0, j = n - 1; i < n; j = i++)
                        {
                            if ((leaf[i][1] > fy) != (leaf[j][1] > fy) &&
                                fx < (leaf[j][0] - leaf[i][0]) * (fy - leaf[i][1]) / (leaf[j][1] - leaf[i][1]) + leaf[i][0])
                                in = !in;
                        }
                        hits += in;
                    }
                }
                uint32_t a = hits * 255 / 16;
                // premultiplied orange
                px[y * size + x] = (a << 24) | ((236 * a / 255) << 16) | ((112 * a / 255) << 8) | (24 * a / 255);
            }
        }
        HBITMAP mask = CreateBitmap(size, size, 1, 1, nullptr);
        ICONINFO ii = { TRUE, 0, 0, mask, color };
        HICON icon = CreateIconIndirect(&ii);
        DeleteObject(mask);
        DeleteObject(color);
        return icon;
    }

    void AddTrayIcon()
    {
        NOTIFYICONDATAW nid = { sizeof nid };
        nid.hWnd = app.tray;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = app.icon;
        wcsncpy_s(nid.szTip, AppName, _TRUNCATE);
        Shell_NotifyIconW(NIM_ADD, &nid);
        UpdateTooltip();
    }

    bool AutostartEnabled()
    {
        return RegGetValueW(HKEY_CURRENT_USER, RunKey, RunValue, RRF_RT_REG_SZ, nullptr, nullptr, nullptr) == ERROR_SUCCESS;
    }

    void SetAutostart(bool on)
    {
        HKEY key;
        if (RegOpenKeyExW(HKEY_CURRENT_USER, RunKey, 0, KEY_SET_VALUE, &key) != ERROR_SUCCESS) return;
        if (on)
        {
            wchar_t path[MAX_PATH];
            GetModuleFileNameW(nullptr, path, MAX_PATH);
            std::wstring cmd = L"\"" + std::wstring(path) + L"\"";
            RegSetValueExW(key, RunValue, 0, REG_SZ, reinterpret_cast<const BYTE*>(cmd.c_str()), DWORD((cmd.size() + 1) * sizeof(wchar_t)));
        }
        else RegDeleteValueW(key, RunValue);
        RegCloseKey(key);
    }

    void Command(UINT id)
    {
        Settings& s = app.settings;
        if (id >= IdMode && id < IdMode + ARRAYSIZE(Modes)) s.mode = Modes[id - IdMode].key;
        else if (id >= IdIntensity && id < IdIntensity + ARRAYSIZE(Intensities)) s.intensity = Intensities[id - IdIntensity];
        else if (id >= IdFps && id < IdFps + ARRAYSIZE(FpsChoices)) s.fps = FpsChoices[id - IdFps];
        else if (id >= IdQuality && id < IdQuality + ARRAYSIZE(QualityScales))
        {
            s.quality = int(id - IdQuality);
            SaveSettings();
            InitGraphics();
            return;
        }
        else if (id == IdIgnoreMax) s.ignoreMaximized = !s.ignoreMaximized;
        else if (id == IdPauseFullscreen) s.pauseOnFullscreen = !s.pauseOnFullscreen;
        else if (id == IdBattery) { s.batterySaver = !s.batterySaver; SaveSettings(); RefreshPower(); return; }
        else if (id == IdRefresh) { app.weather.Refresh(s, false); return; }
        else if (id == IdEdit)
        {
            SaveSettings();
            ShellExecuteW(nullptr, L"open", L"notepad.exe", (L"\"" + Settings::Path() + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
            return;
        }
        else if (id == IdAutostart) { SetAutostart(!AutostartEnabled()); return; }
        else if (id == IdRare) s.rareEvents = !s.rareEvents;
        else if (id >= IdCritter && id <= IdCritter + CritterKinds)
        {
            int kind = int(id - IdCritter);
            app.sim.SpawnCritter(kind == CritterKinds ? -1 : kind, true);
            return;
        }
        else if (id == IdExit) { DestroyWindow(app.tray); return; }
        else return;
        SaveSettings();
        RefreshPacing();
        app.trackDirty = true;
    }

    void ShowMenu()
    {
        const Settings& s = app.settings;
        HMENU m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING | MF_GRAYED, 0, app.weatherNow.Describe().c_str());
        AppendMenuW(m, MF_SEPARATOR, 0, nullptr);

        HMENU modes = CreatePopupMenu();
        for (UINT i = 0; i < ARRAYSIZE(Modes); i++)
            AppendMenuW(modes, MF_STRING | (s.mode == Modes[i].key ? MF_CHECKED : 0), IdMode + i, Modes[i].label);
        AppendMenuW(m, MF_POPUP, UINT_PTR(modes), L"Efekt");

        HMENU levels = CreatePopupMenu();
        for (UINT i = 0; i < ARRAYSIZE(Intensities); i++)
            AppendMenuW(levels, MF_STRING | (std::fabs(s.intensity - Intensities[i]) < 0.01f ? MF_CHECKED : 0), IdIntensity + i, IntensityLabels[i]);
        AppendMenuW(m, MF_POPUP, UINT_PTR(levels), L"Intensywność");

        HMENU perf = CreatePopupMenu();
        for (UINT i = 0; i < ARRAYSIZE(FpsChoices); i++)
        {
            wchar_t label[32];
            swprintf_s(label, L"%d klatek/s", FpsChoices[i]);
            AppendMenuW(perf, MF_STRING | (s.fps == FpsChoices[i] ? MF_CHECKED : 0), IdFps + i, label);
        }
        AppendMenuW(perf, MF_SEPARATOR, 0, nullptr);
        for (UINT i = 0; i < ARRAYSIZE(QualityScales); i++)
            AppendMenuW(perf, MF_STRING | (s.quality == int(i) ? MF_CHECKED : 0), IdQuality + i, QualityLabels[i]);
        AppendMenuW(perf, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(perf, MF_STRING | (s.batterySaver ? MF_CHECKED : 0), IdBattery, L"Oszczędzaj na baterii (20 kl/s, EcoQoS)");
        AppendMenuW(perf, MF_STRING | (s.pauseOnFullscreen ? MF_CHECKED : 0), IdPauseFullscreen, L"Pauza przy aplikacjach pełnoekranowych");
        AppendMenuW(m, MF_POPUP, UINT_PTR(perf), L"Wydajność");

        HMENU rare = CreatePopupMenu();
        AppendMenuW(rare, MF_STRING | (s.rareEvents ? MF_CHECKED : 0), IdRare, L"Pojawiają się same (rzadko, zależnie od pogody)");
        AppendMenuW(rare, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(rare, MF_STRING, IdCritter + CritterKinds, L"Przywołaj losowe teraz");
        for (UINT i = 0; i < CritterKinds; i++) AppendMenuW(rare, MF_STRING, IdCritter + i, CritterLabels[i]);
        AppendMenuW(m, MF_POPUP, UINT_PTR(rare), L"Rzadkie zjawiska");

        AppendMenuW(m, MF_STRING | (s.ignoreMaximized ? MF_CHECKED : 0), IdIgnoreMax, L"Przelatuj przez zmaksymalizowane okna");
        AppendMenuW(m, MF_STRING, IdRefresh, L"Odśwież pogodę");
        AppendMenuW(m, MF_STRING, IdEdit, L"Ustawienia (miasto, współrzędne)…");
        AppendMenuW(m, MF_STRING | (AutostartEnabled() ? MF_CHECKED : 0), IdAutostart, L"Uruchamiaj razem z Windows");
        AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(m, MF_STRING, IdExit, L"Zakończ");

        POINT pt;
        GetCursorPos(&pt);
        SetForegroundWindow(app.tray);
        // TrackPopupMenu runs its own modal loop; a timer keeps frames coming while the menu is open
        SetTimer(app.tray, 1, 8, nullptr);
        UINT cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, app.tray, nullptr);
        KillTimer(app.tray, 1);
        PostMessageW(app.tray, WM_NULL, 0, 0);
        DestroyMenu(m);
        if (cmd) Command(cmd);
    }

    // ------------------------------------------------------------ frame loop

    void Schedule(double seconds)
    {
        double wait = std::max(0.001, app.frameStart + seconds - Now());
        LARGE_INTEGER due;
        due.QuadPart = -LONGLONG(wait * 1e7);
        SetWaitableTimer(app.timer, &due, 0, nullptr, nullptr, FALSE);
    }

    void Housekeeping()
    {
        FILETIME t = IniTime();
        if (CompareFileTime(&t, &app.iniTime) != 0)
        {
            Settings old = app.settings;
            app.iniTime = t;
            app.settings.Load();
            const Settings& s = app.settings;
            if (s.city != old.city || s.latitude != old.latitude || s.longitude != old.longitude) app.weather.Refresh(s, true);
            if (s.quality != old.quality) InitGraphics();
            RefreshPower();
            app.trackDirty = true;
        }
        if (RefreshMonitors()) InitGraphics();
        RefreshPacing();
#ifdef OVERLAY_DEBUG
        {
            wchar_t line[512];
            swprintf_s(line, L"gfx=%d locked=%d off=%d fsfg=%d wins=%d parts=%d screen=%d,%d,%d,%d | %s\n", app.graphicsOk, app.locked,
                       app.displayOff, app.tracker.fullscreenForeground, int(app.tracker.all.size()), int(app.sim.parts.size()),
                       app.screen.left, app.screen.top, app.screen.right, app.screen.bottom, app.weatherNow.Describe().c_str());
            OutputDebugStringW(line);
            if (g_profile.frames > 0)
            {
                static const wchar_t* names[] = { L"okna", L"kursor", L"podmuchy", L"czastki", L"kopczyki", L"zwierzeta", L"render", L"odstep" };
                std::wstring prof = L"  ms avg/max:";
                for (int i = 0; i < 8; i++)
                {
                    wchar_t part[64];
                    swprintf_s(part, L" %s %.2f/%.2f", names[i], g_profile.sum[i] / g_profile.frames, g_profile.peak[i]);
                    prof += part;
                }
                swprintf_s(line, L"%s frames=%d late=%d events=%d\n", prof.c_str(), g_profile.frames, g_profile.late, g_profile.events);
                g_profile = Profile();
            }
            FILE* f = nullptr;
            if (_wfopen_s(&f, (Settings::Path() + L".log").c_str(), L"a, ccs=UTF-8") == 0 && f) { fputws(line, f); fclose(f); }
        }
#endif
        // other topmost windows may have been raised above us
        SetWindowPos(app.overlay, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }

    // One GetCursorPos per frame: no mouse hook, so pointer movement never wakes the process by itself.
    CursorWind SampleCursor(float dt)
    {
        CursorWind c;
        POINT pt;
        if (!GetCursorPos(&pt)) { app.haveCursor = false; return c; }
        float x = float(pt.x - app.screen.left), y = float(pt.y - app.screen.top);
        if (app.haveCursor && dt > 0)
        {
            c.x0 = app.cursorX; c.y0 = app.cursorY; c.x1 = x; c.y1 = y;
            float dx = x - c.x0, dy = y - c.y0;
            // ignore jumps (pointer warps, monitor switches) that are not real motion
            if (std::fabs(dx) + std::fabs(dy) < 600.0f * app.sim.S)
            {
                // the pointer is sampled once a frame, so its speed is a bit ragged: smooth it a little
                app.cursorVX += (dx / dt - app.cursorVX) * 0.5f;
                app.cursorVY += (dy / dt - app.cursorVY) * 0.5f;
                c.vx = app.cursorVX;
                c.vy = app.cursorVY;
                c.valid = true;
            }
        }
        app.cursorX = x;
        app.cursorY = y;
        app.haveCursor = true;
        return c;
    }

    void ClearOnce()
    {
        app.haveCursor = false;
        if (!app.cleared && app.graphicsOk) app.graphicsOk = app.renderer.Clear();
        app.cleared = true;
    }

    void Frame()
    {
        double now = Now();
        app.frameStart = now;
        float dt = float(std::min(0.05, now - app.lastFrame));
        app.lastFrame = now;

        if (now - app.lastHousekeeping > 2.0)
        {
            app.lastHousekeeping = now;
            Housekeeping();
        }

        if (!app.graphicsOk || app.locked || app.displayOff || (app.settings.mode == L"Off" && app.sim.Idle()))
        {
            ClearOnce();
            Schedule(1.0);
            return;
        }

        // window positions come from WinEvent hooks; enumerate only when something changed,
        // every frame while a window is dragged, and twice a second as a safety net
        PROFILE_START(p0);
        if (app.trackDirty || app.moving || now - app.lastTrack > 0.5)
        {
            app.tracker.Update(app.overlay, app.screen, app.settings.ignoreMaximized, app.monitors);
#ifdef OVERLAY_DEBUG
            g_profile.enums++;
#endif
            app.trackDirty = false;
            app.lastTrack = now;
        }
        else app.tracker.Still();
        PROFILE(0, p0);

        if (app.settings.pauseOnFullscreen && app.tracker.fullscreenForeground)
        {
            ClearOnce();
            Schedule(0.5);
            return;
        }

        app.sim.rareEvents = app.settings.rareEvents;
#ifdef OVERLAY_DEBUG
        static double debugStart = now;
        static bool summoned = false;
        if (!summoned && now - debugStart > 3)
        {
            summoned = true;
            for (int kind = 0; kind < CritterKinds; kind++) app.sim.SpawnCritter(kind, true);
        }
#endif
        CursorWind pointer = SampleCursor(dt);
#ifdef OVERLAY_DEBUG
        {
            // a scripted broom for testing heaps without touching the real mouse: sweeps right along the
            // primary taskbar, returns high above it, repeats
            static double sweepStart = now + 25;
            static float px = 0, py = 0;
            double t = now - sweepStart;
            if (t > 0 && t < 14)
            {
                // fast back-and-forth sweeps right over the leaves, like a real hand on the mouse (~3000 px/s)
                float gx = float(-app.screen.left) + 1900, gy = float(GetSystemMetrics(SM_CYSCREEN)) - 70;
                float x = gx + 600 * float(std::sin(t * 5.0)), y = gy + 15 * float(std::sin(t * 13.0));
                if (px != 0)
                {
                    pointer.x0 = px; pointer.y0 = py; pointer.x1 = x; pointer.y1 = y;
                    pointer.vx = (x - px) / dt; pointer.vy = (y - py) / dt;
                    pointer.valid = true;
                }
                px = x; py = y;
            }
        }
#endif
        app.sim.Step(dt, app.tracker, ComputeMix(app.settings, app.weatherNow), pointer);
        if (app.sim.Idle())
        {
            ClearOnce();
            Schedule(0.5);
            return;
        }
#ifdef OVERLAY_DEBUG
        {
            static double lastFrameMs = 0;
            double nowMs = ProfileNow();
            if (lastFrameMs > 0)
            {
                PROFILE(7, lastFrameMs);
                if (nowMs - lastFrameMs > 1000.0 * app.renderer.syncInterval / app.refreshHz * 1.4) g_profile.late++;
            }
            lastFrameMs = nowMs;
        }
#endif
        PROFILE_START(p6);
        if (!app.renderer.Render(app.sim, app.tracker)) { InitGraphics(); RefreshPacing(); }
        PROFILE(6, p6);
#ifdef OVERLAY_DEBUG
        g_profile.frames++;
#endif
        app.cleared = false;
        // Present already waited for the display, so go straight on to the next frame
        Schedule(0);
    }

    void CALLBACK OnWinEvent(HWINEVENTHOOK, DWORD ev, HWND h, LONG obj, LONG child, DWORD, DWORD)
    {
        if (obj != OBJID_WINDOW || child != CHILDID_SELF || !h) return;
#ifdef OVERLAY_DEBUG
        g_profile.events++;
#endif
        if (ev == EVENT_SYSTEM_MOVESIZESTART) app.moving = true;
        else if (ev == EVENT_SYSTEM_MOVESIZEEND) app.moving = false;
        app.trackDirty = true;
    }

    void InstallHooks()
    {
        // deliberately no EVENT_OBJECT_LOCATIONCHANGE: it fires on every mouse move system-wide
        const DWORD ranges[][2] = {
            { EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND },
            { EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND },
            { EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND },
            { EVENT_OBJECT_DESTROY, EVENT_OBJECT_REORDER },
            { EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED },
        };
        for (const auto& r : ranges)
        {
            HWINEVENTHOOK hk = SetWinEventHook(r[0], r[1], nullptr, OnWinEvent, 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
            if (hk) app.hooks.push_back(hk);
        }
    }

    LRESULT CALLBACK WndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp)
    {
        if (msg == app.taskbarCreated && msg) { AddTrayIcon(); return 0; }
        switch (msg)
        {
        case WM_TRAY:
            if (LOWORD(lp) == WM_RBUTTONUP || LOWORD(lp) == WM_LBUTTONUP) ShowMenu();
            return 0;
        case WM_TIMER:
            if (WaitForSingleObject(app.timer, 0) == WAIT_OBJECT_0) Frame();
            return 0;
        case WM_WEATHER:
            app.weatherNow = app.weather.Snapshot();
            UpdateTooltip();
            return 0;
        case WM_DISPLAYCHANGE:
            if (h == app.overlay) { RefreshMonitors(); InitGraphics(); }
            return 0;
        case WM_WTSSESSION_CHANGE:
            if (wp == WTS_SESSION_LOCK) app.locked = true;
            else if (wp == WTS_SESSION_UNLOCK) app.locked = false;
            return 0;
        case WM_POWERBROADCAST:
            if (wp == PBT_APMPOWERSTATUSCHANGE) RefreshPower();
            else if (wp == PBT_POWERSETTINGCHANGE)
            {
                auto* ps = reinterpret_cast<POWERBROADCAST_SETTING*>(lp);
                if (ps->PowerSetting == GUID_CONSOLE_DISPLAY_STATE) app.displayOff = ps->Data[0] == 0;
            }
            return TRUE;
        case WM_DESTROY:
            if (h == app.tray) PostQuitMessage(0);
            return 0;
        }
        return DefWindowProcW(h, msg, wp, lp);
    }
}

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR, int)
{
    HANDLE single = CreateMutexW(nullptr, TRUE, L"Local\\Parapet.SingleInstance");
    if (GetLastError() == ERROR_ALREADY_EXISTS) return 0;

    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    // normal priority: the work per frame is tiny, but late wake-ups would make the animation stutter
    SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS);
    LARGE_INTEGER f;
    QueryPerformanceFrequency(&f);
    app.freq = double(f.QuadPart);

    app.settings.Load();
    if (GetFileAttributesW(Settings::Path().c_str()) == INVALID_FILE_ATTRIBUTES) app.settings.Save();
    app.iniTime = IniTime();

    WNDCLASSEXW wc = { sizeof wc };
    wc.lpfnWndProc = WndProc;
    wc.hInstance = inst;
    wc.lpszClassName = L"ParapetWnd";
    RegisterClassExW(&wc);

    app.overlay = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED |
                                  WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                                  wc.lpszClassName, L"Parapet", WS_POPUP, 0, 0, 1, 1, nullptr, nullptr, inst, nullptr);
    app.tray = CreateWindowExW(WS_EX_TOOLWINDOW, wc.lpszClassName, L"ParapetTray", WS_POPUP, 0, 0, 0, 0, nullptr, nullptr, inst, nullptr);
    if (!app.overlay || !app.tray) return 1;
    SetLayeredWindowAttributes(app.overlay, 0, 255, LWA_ALPHA);

    RefreshMonitors();
    InitGraphics();
    RefreshPacing();
    ShowWindow(app.overlay, SW_SHOWNOACTIVATE);

    app.taskbarCreated = RegisterWindowMessageW(L"TaskbarCreated");
    app.icon = MakeIcon();
    AddTrayIcon();
    InstallHooks();
    WTSRegisterSessionNotification(app.overlay, NOTIFY_FOR_THIS_SESSION);
    HPOWERNOTIFY displayNotify = RegisterPowerSettingNotification(app.overlay, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
    RefreshPower();

    app.weather.Start(app.tray, app.settings);

    app.timer = CreateWaitableTimerExW(nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
    if (!app.timer) app.timer = CreateWaitableTimerW(nullptr, FALSE, nullptr);
    app.lastFrame = app.frameStart = Now();
    Schedule(0);

    for (bool running = true; running;)
    {
        DWORD r = MsgWaitForMultipleObjectsEx(1, &app.timer, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        if (r == WAIT_OBJECT_0) Frame();
        MSG msg;
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE))
        {
            if (msg.message == WM_QUIT) { running = false; break; }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    NOTIFYICONDATAW nid = { sizeof nid };
    nid.hWnd = app.tray;
    nid.uID = 1;
    Shell_NotifyIconW(NIM_DELETE, &nid);
    for (HWINEVENTHOOK hk : app.hooks) UnhookWinEvent(hk);
    if (displayNotify) UnregisterPowerSettingNotification(displayNotify);
    WTSUnRegisterSessionNotification(app.overlay);
    app.weather.Stop();
    app.renderer.Release();
    DestroyWindow(app.overlay);
    if (app.icon) DestroyIcon(app.icon);
    CloseHandle(app.timer);
    CloseHandle(single);
    return 0;
}
