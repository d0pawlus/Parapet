#include "app.h"
#include <winhttp.h>
#include <cstdlib>
#include <algorithm>

// ---------------------------------------------------------------- settings.ini

std::wstring Settings::Path()
{
    wchar_t buf[MAX_PATH];
    DWORD n = GetModuleFileNameW(nullptr, buf, MAX_PATH);
    std::wstring p(buf, n);
    return p.substr(0, p.find_last_of(L'\\') + 1) + L"settings.ini";
}

static std::wstring ReadIni(const std::wstring& file, const wchar_t* key, const std::wstring& def)
{
    wchar_t buf[256];
    GetPrivateProfileStringW(L"Overlay", key, def.c_str(), buf, 256, file.c_str());
    return buf;
}

static void WriteIni(const std::wstring& file, const wchar_t* key, const std::wstring& value)
{
    WritePrivateProfileStringW(L"Overlay", key, value.c_str(), file.c_str());
}

void Settings::Load()
{
    std::wstring f = Path();
    Settings d;
    mode = ReadIni(f, L"Mode", d.mode);
    intensity = std::max(0.1f, std::min(5.0f, float(_wtof(ReadIni(f, L"Intensity", L"1").c_str()))));
    city = ReadIni(f, L"City", L"");
    latitude = ReadIni(f, L"Latitude", L"");
    longitude = ReadIni(f, L"Longitude", L"");
    fps = std::max(10, std::min(120, _wtoi(ReadIni(f, L"Fps", L"30").c_str())));
    quality = std::max(0, std::min(3, _wtoi(ReadIni(f, L"Quality", L"2").c_str())));
    ignoreMaximized = ReadIni(f, L"IgnoreMaximized", L"1") != L"0";
    pauseOnFullscreen = ReadIni(f, L"PauseOnFullscreen", L"1") != L"0";
    batterySaver = ReadIni(f, L"BatterySaver", L"1") != L"0";
    rareEvents = ReadIni(f, L"RareEvents", L"1") != L"0";
}

void Settings::Save() const
{
    std::wstring f = Path();
    wchar_t buf[32];
    WriteIni(f, L"Mode", mode);
    swprintf_s(buf, L"%.2f", intensity); WriteIni(f, L"Intensity", buf);
    WriteIni(f, L"City", city);
    WriteIni(f, L"Latitude", latitude);
    WriteIni(f, L"Longitude", longitude);
    swprintf_s(buf, L"%d", fps); WriteIni(f, L"Fps", buf);
    swprintf_s(buf, L"%d", quality); WriteIni(f, L"Quality", buf);
    WriteIni(f, L"IgnoreMaximized", ignoreMaximized ? L"1" : L"0");
    WriteIni(f, L"PauseOnFullscreen", pauseOnFullscreen ? L"1" : L"0");
    WriteIni(f, L"BatterySaver", batterySaver ? L"1" : L"0");
    WriteIni(f, L"RareEvents", rareEvents ? L"1" : L"0");
}

// ---------------------------------------------------------------- tiny HTTP + JSON helpers

static bool HttpGet(const wchar_t* host, const std::wstring& path, bool https, std::string& body, std::wstring& error)
{
    body.clear();
    HINTERNET s = WinHttpOpen(L"Parapet/1.0", WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, nullptr, nullptr, 0);
    if (!s) { error = L"WinHttpOpen"; return false; }
    WinHttpSetTimeouts(s, 5000, 5000, 10000, 10000);
    HINTERNET c = WinHttpConnect(s, host, https ? INTERNET_DEFAULT_HTTPS_PORT : INTERNET_DEFAULT_HTTP_PORT, 0);
    HINTERNET r = c ? WinHttpOpenRequest(c, L"GET", path.c_str(), nullptr, nullptr, nullptr, https ? WINHTTP_FLAG_SECURE : 0) : nullptr;
    bool ok = r && WinHttpSendRequest(r, nullptr, 0, nullptr, 0, 0, 0) && WinHttpReceiveResponse(r, nullptr);
    if (ok)
    {
        DWORD status = 0, len = sizeof status;
        WinHttpQueryHeaders(r, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, nullptr, &status, &len, nullptr);
        DWORD avail = 0;
        while (WinHttpQueryDataAvailable(r, &avail) && avail)
        {
            size_t old = body.size();
            body.resize(old + avail);
            DWORD read = 0;
            if (!WinHttpReadData(r, &body[old], avail, &read)) break;
            body.resize(old + read);
        }
        ok = status == 200;
        if (!ok) error = L"HTTP " + std::to_wstring(status);
    }
    else error = L"brak połączenia z " + std::wstring(host);
    if (r) WinHttpCloseHandle(r);
    if (c) WinHttpCloseHandle(c);
    WinHttpCloseHandle(s);
    return ok;
}

static bool JsonNumber(const std::string& json, const char* key, double& out, size_t from = 0)
{
    std::string k = std::string("\"") + key + "\":";
    size_t p = json.find(k, from);
    if (p == std::string::npos) return false;
    const char* s = json.c_str() + p + k.size();
    char* end;
    double v = strtod(s, &end);
    if (end == s) return false;
    out = v;
    return true;
}

static std::wstring JsonString(const std::string& json, const char* key)
{
    std::string k = std::string("\"") + key + "\":\"";
    size_t p = json.find(k);
    if (p == std::string::npos) return L"";
    p += k.size();
    size_t e = json.find('"', p);
    std::string v = json.substr(p, e - p);
    int n = MultiByteToWideChar(CP_UTF8, 0, v.c_str(), int(v.size()), nullptr, 0);
    std::wstring w(n, 0);
    MultiByteToWideChar(CP_UTF8, 0, v.c_str(), int(v.size()), &w[0], n);
    return w;
}

static std::wstring UrlEncode(const std::wstring& s)
{
    int n = WideCharToMultiByte(CP_UTF8, 0, s.c_str(), int(s.size()), nullptr, 0, nullptr, nullptr);
    std::string u(n, 0);
    WideCharToMultiByte(CP_UTF8, 0, s.c_str(), int(s.size()), &u[0], n, nullptr, nullptr);
    std::wstring out;
    wchar_t buf[4];
    for (unsigned char ch : u)
    {
        if (isalnum(ch) || ch == '-' || ch == '_' || ch == '.') out += wchar_t(ch);
        else { swprintf_s(buf, L"%%%02X", ch); out += buf; }
    }
    return out;
}

// ---------------------------------------------------------------- weather service

std::wstring WeatherState::Describe() const
{
    if (!ok) return L"Pogoda: brak danych" + (error.empty() ? L"" : L" (" + error + L")");
    const wchar_t* d = L"Pogoda";
    if (code == 0) d = L"bezchmurnie";
    else if (code <= 2) d = L"częściowe zachmurzenie";
    else if (code == 3) d = L"pochmurno";
    else if (code == 45 || code == 48) d = L"mgła";
    else if (code >= 51 && code <= 55) d = L"mżawka";
    else if (code == 56 || code == 57 || code == 66 || code == 67) d = L"marznący deszcz";
    else if (code == 61) d = L"słaby deszcz";
    else if (code == 63) d = L"deszcz";
    else if (code == 65) d = L"ulewa";
    else if (code == 71) d = L"słaby śnieg";
    else if (code == 73) d = L"śnieg";
    else if (code == 75) d = L"intensywny śnieg";
    else if (code == 77) d = L"ziarna śniegu";
    else if (code >= 80 && code <= 82) d = L"przelotny deszcz";
    else if (code == 85 || code == 86) d = L"przelotny śnieg";
    else if (code >= 95) d = L"burza";
    wchar_t buf[256];
    swprintf_s(buf, L"%s: %.0f°C, %s, wiatr %.0f km/h", place.c_str(), tempC, d, windKmh);
    return buf;
}

void WeatherService::Start(HWND notify, const Settings& s)
{
    notifyWnd = notify;
    settings = s;
    running = true;
    wake = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    thread = CreateThread(nullptr, 0, ThreadProc, this, 0, nullptr);
    // weather work is rare and latency-insensitive: keep it off the performance cores
    SetThreadPriority(thread, THREAD_MODE_BACKGROUND_BEGIN);
}

void WeatherService::Stop()
{
    running = false;
    if (wake) SetEvent(wake);
    if (thread) { WaitForSingleObject(thread, 3000); CloseHandle(thread); }
    if (wake) CloseHandle(wake);
    thread = wake = nullptr;
}

void WeatherService::Refresh(const Settings& s, bool relocate)
{
    {
        std::lock_guard<std::mutex> g(lock);
        settings = s;
        if (relocate) relocateRequested = true;
    }
    if (wake) SetEvent(wake);
}

WeatherState WeatherService::Snapshot()
{
    std::lock_guard<std::mutex> g(lock);
    return state;
}

DWORD WINAPI WeatherService::ThreadProc(void* self)
{
    static_cast<WeatherService*>(self)->Run();
    return 0;
}

void WeatherService::Run()
{
    while (running)
    {
        WeatherState w = Fetch();
        {
            std::lock_guard<std::mutex> g(lock);
            // keep showing the last good reading through short outages
            if (w.ok || !state.ok) state = w;
            else state.error = w.error;
        }
        PostMessageW(notifyWnd, WM_APP + 1, 0, 0);
        // 15 minutes normally, 2 minutes after a failure
        WaitForSingleObject(wake, w.ok ? 15 * 60 * 1000 : 2 * 60 * 1000);
    }
}

bool WeatherService::Locate(std::wstring& error)
{
    Settings s;
    {
        std::lock_guard<std::mutex> g(lock);
        s = settings;
        relocateRequested = false;
    }
    if (!s.latitude.empty() && !s.longitude.empty())
    {
        lat = _wtof(s.latitude.c_str());
        lon = _wtof(s.longitude.c_str());
        place = s.city.empty() ? L"Twoja lokalizacja" : s.city;
        return true;
    }
    std::string body;
    if (!s.city.empty())
    {
        if (!HttpGet(L"geocoding-api.open-meteo.com", L"/v1/search?count=1&language=pl&format=json&name=" + UrlEncode(s.city), true, body, error))
            return false;
        if (!JsonNumber(body, "latitude", lat) || !JsonNumber(body, "longitude", lon)) { error = L"nie znaleziono miasta"; return false; }
        place = JsonString(body, "name");
        return true;
    }
    if (!HttpGet(L"ip-api.com", L"/json/?fields=status,city,lat,lon", false, body, error)) return false;
    if (!JsonNumber(body, "lat", lat) || !JsonNumber(body, "lon", lon)) { error = L"nieznana lokalizacja"; return false; }
    place = JsonString(body, "city");
    return true;
}

WeatherState WeatherService::Fetch()
{
    WeatherState w;
    bool relocate;
    {
        std::lock_guard<std::mutex> g(lock);
        relocate = relocateRequested;
    }
    if (relocate && !Locate(w.error))
    {
        std::lock_guard<std::mutex> g(lock);
        relocateRequested = true;
        return w;
    }

    wchar_t path[512];
    swprintf_s(path, L"/v1/forecast?latitude=%.4f&longitude=%.4f&current=temperature_2m,precipitation,rain,snowfall,weather_code,wind_speed_10m,wind_direction_10m",
               lat, lon);
    std::string body;
    if (!HttpGet(L"api.open-meteo.com", path, true, body, w.error)) return w;
    size_t cur = body.find("\"current\":");
    if (cur == std::string::npos) { w.error = L"zła odpowiedź"; return w; }
    double code = 0;
    JsonNumber(body, "temperature_2m", w.tempC, cur);
    JsonNumber(body, "precipitation", w.precip, cur);
    JsonNumber(body, "rain", w.rain, cur);
    JsonNumber(body, "snowfall", w.snowfall, cur);
    JsonNumber(body, "weather_code", code, cur);
    JsonNumber(body, "wind_speed_10m", w.windKmh, cur);
    JsonNumber(body, "wind_direction_10m", w.windDir, cur);
    w.code = int(code);
    w.place = place;
    w.ok = true;
    return w;
}

// ---------------------------------------------------------------- weather -> effects

EffectMix ComputeMix(const Settings& s, const WeatherState& w)
{
    EffectMix m;
    if (w.ok)
    {
        m.tempC = float(w.tempC);
        // meteorological direction is where the wind comes from; westerly wind blows to the right
        m.windPx = float(-std::sin(w.windDir * 3.14159265 / 180.0) * std::min(w.windKmh, 70.0) * 5.0);
    }
    else m.windPx = 30;

    const std::wstring& mode = s.mode;
    if (mode == L"Leaves") m.leaves = 1;
    else if (mode == L"Rain") m.rain = 1;
    else if (mode == L"Snow") m.snow = 1;
    else if (mode == L"Storm")
    {
        m.leaves = 1.3f;
        m.rain = 1.4f;
        m.thunder = true;
        if (std::fabs(m.windPx) < 150) m.windPx = m.windPx < 0 ? -170.0f : 170.0f;
    }
    else if (mode == L"Off") {}
    else if (!w.ok) m.leaves = 1;
    else
    {
        int c = w.code;
        bool snowCode = (c >= 71 && c <= 77) || c == 85 || c == 86;
        bool rainCode = (c >= 51 && c <= 67) || (c >= 80 && c <= 82) || c >= 95;
        SYSTEMTIME now;
        GetLocalTime(&now);
        bool autumn = now.wMonth >= 9 && now.wMonth <= 11;
        bool winter = now.wMonth == 12 || now.wMonth <= 2;

        if (snowCode || (w.precip > 0.05 && w.tempC <= 0.5))
        {
            m.snow = c == 75 || c == 86 ? 1.6f : c == 73 ? 1.0f : 0.55f;
        }
        else if (rainCode || w.precip > 0.05)
        {
            bool heavy = c == 55 || c == 65 || c == 67 || c == 82 || c >= 95 || w.precip > 4;
            bool medium = c == 53 || c == 63 || c == 81 || w.precip > 1;
            m.rain = heavy ? 1.5f : medium ? 0.9f : 0.45f;
            m.thunder = c >= 95;
            if (autumn) m.leaves = 0.4f;
        }
        else
        {
            m.leaves = 0.6f + float(std::min(1.0, w.windKmh / 40.0)) * 0.9f;
            if (winter && w.tempC <= 2) m.leaves = 0.15f;
        }
    }
    m.leaves *= s.intensity;
    m.rain *= s.intensity;
    m.snow *= s.intensity;
    return m;
}
