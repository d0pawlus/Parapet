#pragma once
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <wrl/client.h>
#include <d3d11.h>
#include <dxgi1_3.h>
#include <d2d1_3.h>
#include <dcomp.h>
#include <cstdint>
#include <cmath>
#include <string>
#include <vector>
#include <mutex>

using Microsoft::WRL::ComPtr;

// Debug-build frame profiler: time per stage, averaged and maxed over the logging interval.
#ifdef OVERLAY_DEBUG
struct Profile { double sum[8] = {}, peak[8] = {}; int frames = 0, events = 0, enums = 0, late = 0; };
extern Profile g_profile;
double ProfileNow();
#define PROFILE(slot, since) do { double e_ = ProfileNow() - (since); g_profile.sum[slot] += e_; if (e_ > g_profile.peak[slot]) g_profile.peak[slot] = e_; } while (0)
#define PROFILE_START(name) double name = ProfileNow()
#else
#define PROFILE(slot, since) do {} while (0)
#define PROFILE_START(name) do {} while (0)
#endif

// ---------------------------------------------------------------- settings

struct Settings
{
    std::wstring mode = L"Auto";   // Auto, Leaves, Rain, Snow, Storm, Off
    float intensity = 1.0f;
    std::wstring city;             // empty = locate by IP
    std::wstring latitude, longitude;
    int fps = 30;
    int quality = 2;               // 0 low (half res), 1 medium (3/4 res), 2 high (full res), 3 ultra (+ leaf shadows)
    bool ignoreMaximized = true;
    bool pauseOnFullscreen = true;
    bool batterySaver = true;
    bool rareEvents = true;        // hedgehogs, foxes, snowdrops and friends now and then

    static std::wstring Path();
    void Load();
    void Save() const;
};

// ---------------------------------------------------------------- weather

struct WeatherState
{
    bool ok = false;
    std::wstring place, error;
    double tempC = 10, precip = 0, rain = 0, snowfall = 0, windKmh = 0, windDir = 270;
    int code = 0;
    std::wstring Describe() const;
};

struct EffectMix
{
    float leaves = 0, rain = 0, snow = 0;
    float windPx = 0;   // px/s at 1080p, positive = blowing right
    float tempC = 10;
    bool thunder = false;
};

EffectMix ComputeMix(const Settings& s, const WeatherState& w);

class WeatherService
{
public:
    void Start(HWND notify, const Settings& s);
    void Stop();
    void Refresh(const Settings& s, bool relocate);
    WeatherState Snapshot();

private:
    static DWORD WINAPI ThreadProc(void* self);
    void Run();
    bool Locate(std::wstring& error);
    WeatherState Fetch();

    std::mutex lock;
    WeatherState state;
    Settings settings;
    bool relocateRequested = true;
    double lat = 0, lon = 0;
    std::wstring place;
    HANDLE thread = nullptr, wake = nullptr;
    volatile bool running = false;
    HWND notifyWnd = nullptr;
};

// ---------------------------------------------------------------- windows on screen

struct Win
{
    HWND h;
    float l, t, r, b;
    float dx, dy;      // movement since the previous update
    int z;             // index in Tracker::all, 0 = topmost
    bool surface;      // false for ignored maximized/fullscreen windows: they hide what is below, nothing lands on them
};

class Tracker
{
public:
    std::vector<Win> all;
    bool fullscreenForeground = false;

    void Update(HWND self, const RECT& screen, bool ignoreMaximized, const std::vector<RECT>& monitors);
    void Still();
    const Win* Get(HWND h) const;
    // surfacesOnly: ignore maximized/fullscreen windows that particles fly over (used for the taskbar ground)
    bool Covered(float x, float y, int z, bool surfacesOnly = false) const;

private:
    std::vector<HWND> handles;
    std::vector<Win> prev;
};

// ---------------------------------------------------------------- simulation

// The mouse pointer's path during the last frame, in overlay coordinates; a moving pointer is a gust of wind.
// A puff of air shed by the moving pointer: it keeps travelling ahead of the pointer, widens and dies away.
struct Gust
{
    float x, y, vx, vy, r, age, life, swirl;
};

struct CursorWind
{
    float x0 = 0, y0 = 0, x1 = 0, y1 = 0;
    float vx = 0, vy = 0;
    bool valid = false;
};

enum Kind : uint8_t { KLeaf, KRain, KSnow, KSplash, KRipple };
enum PState : uint8_t { Falling, Resting, Fading };
enum LeafFlight : uint8_t { Pendulum, Tumble, Spiral };

struct Particle
{
    float x, y, vx, vy;
    float rot, rotv, phase, freq;
    float size, depth, age, life, alpha;
    float offx, rest;
    // leaf flight: a second (flip) oscillator, swing amplitude, drag, tilt and the accumulated spin
    float flip, phase2, freq2, amp, drag, tilt, spin, modeTime, noise, fall;
    float landed;   // when it came to rest
    uint32_t landSeq;   // unique landing order, so leaves that land in the same frame still stack the same way every frame
    // settling down after landing: progress 0..1 from the falling pose (startRot, startFlip) to lying flat (restRot)
    float settle, startRot, restRot, startFlip;
    float heapVol, heapVol0;   // volume this leaf adds to a heap, and what it started with (it shrinks while fading)
    float massFrac;            // where in the heap's mass it sits (0 = left end, 1 = right end): it moves as the heap reshapes
    HWND host;
    uint8_t kind, state, shape, color, mode;   // color doubles as splash tint: 0 water, 1 mud
    bool onGround, hidden;
    bool heaped;   // part of a heap swept up by the pointer: sits on the heap's surface at depth vy (0..1)
};

// Everything that piles up on a surface (a window's top edge or the taskbar), per 4 px bucket:
// snow, mud, standing water and a small 1D wave simulation on the water.
struct SnowCap
{
    static const int Bucket = 4;
    HWND h = nullptr;
    std::vector<float> height, water, mud, wave, waveV, terrain;
    std::vector<float> pile;   // height of the leaf heap, rebuilt every frame from the resting leaves
    float max = 14, waterMax = 6, mudMax = 0, lastSeen = 0;
    bool drains = true;   // windows spill water over their ends; the taskbar keeps it
    float beadL = 0, beadR = 0;   // drops gathering at a window's top corners before they fall
    float corniceL = 0, corniceR = 0;   // snow creeping over a window's corners, hanging there until it drops
    float stirred = 0;            // the pointer swept this surface's leaf heap recently (seconds left, counts down)
    float stirL = 0, stirR = 0;   // ...and where: only leaves in that stretch follow the moving heap

    void Init(int width, float maxHeight, float maxWater, float maxMud);
    void Resize(int width);
    bool Any() const;
    bool AnySnow() const;
    bool AnyWater() const;
    bool AnyMud() const;
    float At(float rel) const;
    void Add(float rel, float amount);
    void AddWater(float rel, float amount);
    void AddMud(float rel, float amount);
    void Relax(float dt, float melt);
    // returns water spilled over the left/right ends (windows drip, the taskbar does not)
    void Flow(float dt, float evaporate, float dry, float& spillLeft, float& spillRight);
};

struct LeafShape
{
    std::vector<D2D1_POINT_2F> outline, veinFrom, veinTo;   // vein 0 is the midrib
    D2D1_POINT_2F stem;
};

void BuildLeafShape(int species, LeafShape& out);

// Rare visitors and plants. Each appears only now and then, and only when the weather suits it.
enum CritterKind : uint8_t { Hedgehog, Caterpillar, SnowFox, Snowdrops, Snail, Frog, Robin, Squirrel, Mushrooms, CritterKinds };

struct Critter
{
    uint8_t kind, state;
    float x, y, vx, vy;
    float dir;          // +1 facing right, -1 facing left
    float t, timer;     // age and time left in the current state
    float anim;         // gait / flap / pulse phase
    float grow;         // plants: 0..1, others: generic progress
    float alpha, angle, seed;
    float face;         // drawn facing, eases towards dir so turning around is a smooth flip
    float blink;        // seconds to the next blink; negative while the eyes are shut
    float look;         // head turned towards the pointer, -1..1 in screen terms
    float squash;       // landing / crouching squash, 0..1
    float aux, aux2;    // per-kind extras: tail flicks, tongue, wiggles
    int count;          // leaves eaten or carried, hops made
    uint8_t carried[4]; // leaf colours on the hedgehog's spines
    HWND host;          // robin: the window it sits on
    float offx;
    bool hidden;
    std::vector<D2D1_POINT_2F> trail;   // snail slime
    std::vector<float> trailAge;
};

class Sim
{
public:
    static const int LeafShapes = 28, LeafColors = 8;

    std::vector<Particle> parts;
    std::vector<SnowCap> caps;
    SnowCap ground;
    std::vector<float> groundY;       // per SnowCap bucket
    std::vector<float> groundScale;   // per bucket: pixel scale of the monitor there (1 = 1080p)
    std::vector<int> restOrder;       // resting leaves bottom to top, for drawing heaps in the right order
    std::vector<Critter> critters;
    bool rareEvents = true;
    float flash = 0, S = 1;
    int W = 0, H = 0;

    void Reset(int width, int height, float scale);
    void SetGround(const std::vector<RECT>& workAreas, const std::vector<RECT>& monitors);
    float ScaleAt(float x) const;   // monitor-local pixel scale, so a 1080p screen next to a 4K one gets 1080p-sized things
    void Step(float dt, const Tracker& tr, const EffectMix& mix, const CursorWind& cursor);
    bool Idle() const;
    float GroundAt(float x) const;
    const SnowCap* CapOf(HWND h) const;
    void SpawnCritter(int kind, bool forced);
    float SurfaceY(float x) const;   // top of the taskbar ground (mud and snow, not water)

private:
    float Rand() { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; return (seed & 0xFFFFFF) / 16777216.0f; }
    float Range(float a, float b) { return a + (b - a) * Rand(); }
    float Wind() const;
    float SpawnX(float wind);
    Particle NewLeaf(float wind);
    Particle NewRain(float wind);
    Particle NewSnow(float wind);
    void PickFlight(Particle& p);
    void UpdateLeafFlight(Particle& p, float dt, float wind);
    void Seed(int kind, int count, float wind, const Tracker& tr);
    bool Update(Particle& p, float dt, float wind, const Tracker& tr);
    bool UpdateResting(Particle& p, float dt, const Tracker& tr);
    bool Land(Particle& p, const Win* w, float y);
    bool FindLanding(float x, float y0, float y1, const Tracker& tr, const Win*& hit, float& hy) const;
    bool Swallowed(float x, float y, const Tracker& tr) const;
    float CapAt(const Win& w, float x) const;
    SnowCap& CapFor(const Win& w);
    bool Fast(const Win& w, float dt) const;
    void UpdateCaps(float dt, const Tracker& tr);
    void SettleSnow(SnowCap& c, float left, bool isGround, float dt);
    void SlideSnowOffEdges(SnowCap& c, const Win& w, float dt);
    void Splash(float x, float y, float depth, int tint = 0, int count = 0, float power = 1);
    void Ripple(float x, float y, float depth);
    void Drip(const Win& w, int side);
    float CursorFalloff(const CursorWind& c, float x, float y, float radius) const;
    void BlowCap(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt);
    void StirWater(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt);
    void ApplyCursor(const CursorWind& cursor, float dt, const Tracker& tr);
    void ApplyGusts(const CursorWind& cursor, float dt);

    void StackLeaves(const Tracker& tr);
    float HeapAt(const SnowCap& c, float rel) const;
    void JoinHeap(Particle& p, SnowCap& c, float left, float depth, bool addVolume);
    void TransportHeap(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt);
    void UpdateCritters(float dt, const Tracker& tr);
    bool UpdateCritter(Critter& c, float dt, const Tracker& tr);
    bool CursorNear(float x, float y, float radius) const;
    int NearestGroundLeaf(float x, float range) const;
    int RestingGroundLeaves() const;
    bool PuddleAt(float x, float& depth) const;
    float FindPuddle(float from, float dir) const;

    std::vector<Particle> pending;
    CursorWind cursor;
    const Tracker* tracker = nullptr;
    std::vector<Gust> gusts;
    float gustClock = 0;
    float critterClock = 0;
    float stepDt = 1 / 60.0f;
    uint32_t landCounter = 0;
    std::vector<float> heapTmp;
    // stretches of the taskbar drift where the snow is being blown away for a few seconds
    struct SnowGust { float x, w, strength, life, age; };
    std::vector<SnowGust> snowGusts;
    EffectMix mix, prevMix;
    uint32_t seed = 0x9E3779B9u;
    float time = 0, leafAcc = 0, rainAcc = 0, snowAcc = 0;
    int resting = 0;
};

// ---------------------------------------------------------------- rendering

class Renderer
{
public:
    bool Init(HWND hwnd, int width, int height, float renderScale, bool leafShadows);
    void Release();
    bool Render(const Sim& sim, const Tracker& tr);
    bool Clear();
    // show each frame for this many display refreshes, so motion is even (60 fps on a 160 Hz screen judders)
    UINT syncInterval = 1;

private:
    bool BuildAtlas();
    void BuildSprites(const Sim& sim, const Tracker& tr);
    void DrawCaps(const Sim& sim, const Tracker& tr);
    void DrawCritters(const Sim& sim, const Tracker& tr);
    void Fill(float cx, float cy, float rx, float ry, D2D1_COLOR_F c);
    void Stroke(float x0, float y0, float x1, float y1, float w, D2D1_COLOR_F c);
    void Poly(const D2D1_POINT_2F* pts, int n, D2D1_COLOR_F c);
    ComPtr<ID2D1SolidColorBrush> paint;

    // Surface layers (snow, mud, water) are sprites too: a column per bucket plus a rotated segment along the top
    // edge, in their own batch drawn before the visitors and particles.
    ComPtr<ID2D1SpriteBatch> layerBatch;
    std::vector<D2D1_RECT_F> ldest;
    std::vector<D2D1_RECT_U> lsrc;
    std::vector<D2D1_COLOR_F> lcolors;
    std::vector<D2D1_MATRIX_3X2_F> ltransforms;

    ComPtr<ID3D11Device> d3d;
    ComPtr<IDXGISwapChain1> swap;
    ComPtr<ID2D1Factory1> factory;
    ComPtr<ID2D1Device> d2dDevice;
    ComPtr<ID2D1DeviceContext3> dc;
    ComPtr<ID2D1Bitmap1> target, atlas;
    ComPtr<ID2D1SpriteBatch> batch;
    ComPtr<ID2D1SolidColorBrush> flashBrush;
    ComPtr<IDCompositionDevice> dcomp;
    ComPtr<IDCompositionTarget> dcompTarget;
    ComPtr<IDCompositionVisual> visual;

    std::vector<D2D1_RECT_F> dest;
    std::vector<D2D1_RECT_U> src;
    std::vector<D2D1_COLOR_F> colors;
    std::vector<D2D1_MATRIX_3X2_F> transforms;
    float k = 1;
    bool shadows = false;
};
