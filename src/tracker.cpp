#include "app.h"
#include <dwmapi.h>

static BOOL CALLBACK CollectWindow(HWND h, LPARAM list)
{
    reinterpret_cast<std::vector<HWND>*>(list)->push_back(h);
    return TRUE;
}

void Tracker::Update(HWND self, const RECT& screen, bool ignoreMaximized, const std::vector<RECT>& monitors)
{
    handles.clear();
    EnumWindows(CollectWindow, reinterpret_cast<LPARAM>(&handles));

    prev.swap(all);
    all.clear();
    fullscreenForeground = false;
    HWND fg = GetForegroundWindow();
    wchar_t buf[64];

    for (HWND h : handles)
    {
        if (h == self || !IsWindowVisible(h) || IsIconic(h)) continue;
        LONG ex = GetWindowLongW(h, GWL_EXSTYLE);
        if (ex & (WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT)) continue;
        // InternalGetWindowText never sends messages, so a hung app cannot stall us
        if (InternalGetWindowText(h, buf, 2) == 0) continue;
        DWORD cloaked = 0;
        if (SUCCEEDED(DwmGetWindowAttribute(h, DWMWA_CLOAKED, &cloaked, sizeof cloaked)) && cloaked) continue;
        if (GetClassNameW(h, buf, 64))
        {
            if (!wcscmp(buf, L"Progman") || !wcscmp(buf, L"WorkerW") ||
                !wcscmp(buf, L"Shell_TrayWnd") || !wcscmp(buf, L"Shell_SecondaryTrayWnd")) continue;
        }
        RECT r;
        if (FAILED(DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &r, sizeof r)) && !GetWindowRect(h, &r)) continue;
        if (r.right - r.left < 60 || r.bottom - r.top < 40) continue;

        bool fullscreen = false;
        for (const RECT& m : monitors)
            if (r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom) fullscreen = true;
        if (fullscreen && h == fg) fullscreenForeground = true;

        Win w;
        w.h = h;
        w.l = float(r.left - screen.left);
        w.t = float(r.top - screen.top);
        w.r = float(r.right - screen.left);
        w.b = float(r.bottom - screen.top);
        w.dx = w.dy = 0;
        w.z = int(all.size());
        w.surface = !(ignoreMaximized && (fullscreen || IsZoomed(h)));
        for (const Win& o : prev)
        {
            if (o.h == h) { w.dx = w.l - o.l; w.dy = w.t - o.t; break; }
        }
        all.push_back(w);
    }
}

void Tracker::Still()
{
    for (Win& w : all) w.dx = w.dy = 0;
}

const Win* Tracker::Get(HWND h) const
{
    for (const Win& w : all)
        if (w.h == h) return &w;
    return nullptr;
}

bool Tracker::Covered(float x, float y, int z, bool surfacesOnly) const
{
    int n = z < int(all.size()) ? z : int(all.size());
    for (int i = 0; i < n; i++)
    {
        const Win& w = all[i];
        if (surfacesOnly && !w.surface) continue;
        if (x >= w.l && x <= w.r && y >= w.t && y <= w.b) return true;
    }
    return false;
}
