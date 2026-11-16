#include "app.h"
#include <algorithm>

namespace
{
    const size_t MaxParticles = 7000;
    const int MaxResting = 400;
    const float Pi = 3.14159265f;

    // spawn rates per second for a 1920 px wide screen at intensity 1
    const float LeafRate = 4.0f, RainRate = 380.0f, SnowRate = 45.0f;

    // relative size of each leaf species (plane and magnolia are big, hawthorn and rowan small)
    const float SpeciesSize[Sim::LeafShapes] = {
        1.0f, 1.15f, 0.9f, 1.0f, 1.15f, 0.8f, 1.0f, 0.9f, 0.85f, 1.0f, 1.15f, 0.9f, 1.25f, 0.85f,
        0.85f, 1.0f, 1.0f, 0.85f, 0.85f, 0.95f, 0.7f, 1.1f, 1.0f, 1.15f, 0.8f, 1.25f, 0.7f, 1.05f,
    };
    const int Ginkgo = 13;

    void Resample(std::vector<float>& v, size_t n)
    {
        if (v.size() == n) return;
        std::vector<float> r(n);
        for (size_t i = 0; i < n && !v.empty(); i++) r[i] = v[i * v.size() / n];
        v.swap(r);
    }
}

// ---------------------------------------------------------------- surface layers

void SnowCap::Init(int width, float maxHeight, float maxWater, float maxMud)
{
    size_t n = size_t(std::max(1, width / Bucket));
    for (std::vector<float>* v : { &height, &water, &mud, &wave, &waveV, &terrain, &pile }) v->assign(n, 0.0f);
    max = maxHeight;
    waterMax = maxWater;
    mudMax = maxMud;
}

void SnowCap::Resize(int width)
{
    size_t n = size_t(std::max(1, width / Bucket));
    for (std::vector<float>* v : { &height, &water, &mud, &wave, &waveV, &terrain, &pile }) Resample(*v, n);
}

bool SnowCap::AnySnow() const
{
    for (float h : height)
        if (h > 0.3f) return true;
    return false;
}

bool SnowCap::AnyWater() const
{
    for (float h : water)
        if (h > 0.2f) return true;
    return false;
}

bool SnowCap::AnyMud() const
{
    for (float h : mud)
        if (h > 0.2f) return true;
    return false;
}

bool SnowCap::Any() const
{
    return AnySnow() || AnyWater() || AnyMud();
}

float SnowCap::At(float rel) const
{
    int i = int(rel / Bucket);
    return i >= 0 && i < int(height.size()) ? height[i] + water[i] + mud[i] : 0.0f;
}

void SnowCap::Add(float rel, float amount)
{
    int n = int(height.size());
    int c = int(rel / Bucket);
    for (int d = -1; d <= 1; d++)
    {
        int i = c + d;
        if (i < 0 || i >= n) continue;
        // snow slides off near the edges, so caps taper there
        int edge = std::min(i, n - 1 - i);
        float limit = max * std::min(1.0f, (edge + 1) / 14.0f);
        height[i] = std::min(limit, height[i] + (d == 0 ? amount : amount * 0.5f));
    }
}

void SnowCap::AddWater(float rel, float amount)
{
    static const float spread[] = { 0.3f, 0.6f, 1.0f, 0.6f, 0.3f };
    int n = int(water.size()), c = int(rel / Bucket);
    for (int d = -2; d <= 2; d++)
    {
        int i = c + d;
        if (i >= 0 && i < n) water[i] = std::min(waterMax, water[i] + amount * spread[d + 2]);
    }
}

void SnowCap::AddMud(float rel, float amount)
{
    if (mudMax <= 0) return;
    int n = int(mud.size()), c = int(rel / Bucket);
    for (int d = -1; d <= 1; d++)
    {
        int i = c + d;
        if (i >= 0 && i < n) mud[i] = std::min(mudMax, mud[i] + (d == 0 ? amount : amount * 0.5f));
    }
}

void SnowCap::Relax(float dt, float melt)
{
    for (size_t i = 0; i + 1 < height.size(); i++)
    {
        float d = height[i] - height[i + 1];
        if (d > 2.0f) { float t = (d - 2.0f) * 0.25f; height[i] -= t; height[i + 1] += t; }
        else if (d < -2.0f) { float t = (-d - 2.0f) * 0.25f; height[i] += t; height[i + 1] -= t; }
    }
    float m = melt * dt;
    for (float& h : height) h = std::max(0.0f, h - m);
}

void SnowCap::Flow(float dt, float evaporate, float dry, float& spillLeft, float& spillRight)
{
    spillLeft = spillRight = 0;
    int n = int(water.size());
    auto level = [&](int i) { return terrain[i] + mud[i] + height[i] + water[i]; };

    // water runs towards lower ground; sweep both ways so neither direction is favoured
    for (int pass = 0; pass < 2; pass++)
    {
        for (int k = 0; k + 1 < n; k++)
        {
            int i = pass == 0 ? k : n - 2 - k;
            float d = (level(i) - level(i + 1)) * 0.45f;
            if (d > 0) { float m = std::min(d, water[i]); water[i] -= m; water[i + 1] += m; }
            else { float m = std::min(-d, water[i + 1]); water[i + 1] -= m; water[i] += m; }
        }
    }
    if (drains && n > 1)
    {
        float k = std::min(1.0f, dt * 3.0f);
        spillLeft = water[0] * k;
        spillRight = water[n - 1] * k;
        water[0] -= spillLeft;
        water[n - 1] -= spillRight;
    }
    for (int i = 0; i < n; i++)
    {
        water[i] = std::max(0.0f, water[i] - evaporate * dt);
        mud[i] = std::max(0.0f, mud[i] - dry * dt);
    }

    // ripples travelling across the puddles: a 1D wave equation, sub-stepped to stay stable
    int steps = std::max(1, std::min(4, int(std::ceil(dt * 60.0f))));
    float h = dt / steps;
    for (int s = 0; s < steps; s++)
    {
        for (int i = 0; i < n; i++)
        {
            if (water[i] < 0.2f) { wave[i] = waveV[i] = 0; continue; }
            float l = wave[i > 0 ? i - 1 : i], r = wave[i + 1 < n ? i + 1 : i];
            waveV[i] += ((l + r - 2 * wave[i]) * 900.0f - waveV[i] * 3.0f) * h;
        }
        for (int i = 0; i < n; i++)
        {
            wave[i] += waveV[i] * h;
            float lim = water[i] * 0.8f;
            wave[i] = std::max(-lim, std::min(lim, wave[i]));
        }
    }
}

// ---------------------------------------------------------------- setup

void Sim::Reset(int width, int height, float scale)
{
    W = width;
    H = height;
    S = scale;
    parts.clear();
    parts.reserve(MaxParticles);
    caps.clear();
    critters.clear();
    seed ^= GetTickCount();
    ground.Init(width, 48.0f * scale, 16.0f * scale, 7.0f * scale);
    ground.drains = false;
    // invisible bumps along the taskbar so rain gathers into separate puddles
    float a1 = 2 * Pi * SnowCap::Bucket / (Range(260, 520) * S), a2 = 2 * Pi * SnowCap::Bucket / (Range(90, 180) * S);
    float p1 = Range(0, 6.28f), p2 = Range(0, 6.28f);
    for (size_t i = 0; i < ground.terrain.size(); i++)
        ground.terrain[i] = 5.0f * S * (1.0f + 0.65f * std::sin(i * a1 + p1) + 0.35f * std::sin(i * a2 + p2));
    groundY.assign(ground.height.size(), float(height));
    prevMix = EffectMix();
}

void Sim::SetGround(const std::vector<RECT>& workAreas, const std::vector<RECT>& monitors)
{
    groundScale.assign(groundY.size(), S);
    for (size_t i = 0; i < groundY.size(); i++)
    {
        float x = float(i * SnowCap::Bucket + SnowCap::Bucket / 2);
        float gy = float(H) + 200.0f;   // gaps between monitors have no ground
        bool found = false;
        for (const RECT& r : workAreas)
        {
            if (x >= r.left && x < r.right)
            {
                gy = found ? std::max(gy, float(r.bottom)) : float(r.bottom);
                found = true;
            }
        }
        groundY[i] = gy;
        for (const RECT& m : monitors)
            if (x >= m.left && x < m.right)
                groundScale[i] = std::max(1.0f, std::min(3.0f, (m.bottom - m.top) / 1080.0f));
    }
}

float Sim::ScaleAt(float x) const
{
    if (groundScale.empty()) return S;
    int i = std::max(0, std::min(int(groundScale.size()) - 1, int(x / SnowCap::Bucket)));
    return groundScale[i];
}

bool Sim::Idle() const
{
    if (!parts.empty() || !critters.empty() || flash > 0 || ground.Any()) return false;
    for (const SnowCap& c : caps)
        if (c.Any()) return false;
    return true;
}

float Sim::GroundAt(float x) const
{
    int i = int(x / SnowCap::Bucket);
    if (i < 0) i = 0;
    if (i >= int(groundY.size())) i = int(groundY.size()) - 1;
    return groundY[i];
}

const SnowCap* Sim::CapOf(HWND h) const
{
    for (const SnowCap& c : caps)
        if (c.h == h) return &c;
    return nullptr;
}

float Sim::CapAt(const Win& w, float x) const
{
    const SnowCap* c = CapOf(w.h);
    return c ? c->At(x - w.l) : 0.0f;
}

SnowCap& Sim::CapFor(const Win& w)
{
    for (SnowCap& c : caps)
        if (c.h == w.h) return c;
    caps.emplace_back();
    SnowCap& c = caps.back();
    c.h = w.h;
    c.lastSeen = time;
    c.Init(int(w.r - w.l), 36.0f * S, 5.0f * S, 0);
    // a window's top edge works like a little roof: it falls off towards both corners, so rain runs to the gutters
    size_t n = c.terrain.size();
    for (size_t j = 0; j < n; j++)
        c.terrain[j] = 3.0f * S * (1.0f - std::fabs(2.0f * j / std::max<size_t>(1, n - 1) - 1.0f));
    return c;
}

// ---------------------------------------------------------------- spawning

float Sim::Wind() const
{
    float g = 0.5f * std::sin(time * 0.31f) + 0.3f * std::sin(time * 0.77f + 1.3f) + 0.2f * std::sin(time * 1.73f + 0.5f);
    return (mix.windPx * (0.75f + 0.45f * g) + 25.0f * g) * S;
}

float Sim::SpawnX(float wind)
{
    float margin = std::fabs(wind) * 3.0f;
    return wind > 0 ? Range(-margin, float(W)) : Range(0.0f, float(W) + margin);
}

void Sim::PickFlight(Particle& p)
{
    const float S = ScaleAt(p.x);   // sizes and speeds follow the monitor the leaf is on
    float sign = Rand() < 0.5f ? -1.0f : 1.0f;
    float r = Rand();
    p.modeTime = Range(2.5f, 9.0f);
    if (r < 0.55f)
    {
        // the classic falling leaf: swinging side to side, stalling at each end of the swing
        p.mode = Pendulum;
        p.freq = Range(1.6f, 3.4f);
        p.amp = Range(35.0f, 110.0f) * S * p.depth;
        p.tilt = Range(20, 55);
        p.freq2 = Range(0.2f, 0.9f) * sign;
        p.rotv = Range(-25, 25);
    }
    else if (r < 0.8f)
    {
        // tumbling end over end
        p.mode = Tumble;
        p.freq = Range(2.5f, 5.0f);
        p.amp = Range(10.0f, 35.0f) * S * p.depth;
        p.tilt = Range(5, 20);
        p.freq2 = Range(3.0f, 8.0f) * sign;
        p.rotv = Range(150, 480) * sign;
    }
    else
    {
        // spiralling down around a vertical axis
        p.mode = Spiral;
        p.freq = Range(1.5f, 3.0f);
        p.amp = Range(40.0f, 90.0f) * S * p.depth;
        p.tilt = Range(10, 30);
        p.freq2 = p.freq;
        p.rotv = Range(40, 140) * sign;
    }
}

Particle Sim::NewLeaf(float wind)
{
    Particle p = {};
    p.kind = KLeaf;
    p.x = SpawnX(wind);
    const float S = ScaleAt(p.x);
    p.depth = Range(0.6f, 1.0f);
    p.shape = uint8_t(Rand() * LeafShapes) % LeafShapes;
    p.size = Range(22.0f, 40.0f) * S * p.depth * SpeciesSize[p.shape];
    p.color = uint8_t(Rand() * LeafColors) % LeafColors;
    if (p.shape == Ginkgo) p.color = Rand() < 0.5f ? 2 : 6;   // ginkgo turns butter yellow
    p.y = -p.size;
    p.vx = wind;
    p.vy = 40.0f * S;
    p.phase = Range(0, 2 * Pi);
    p.phase2 = Range(0, 2 * Pi);
    p.spin = Range(0, 360);
    p.drag = Range(0.8f, 2.2f);
    p.fall = Range(45.0f, 85.0f) * S * p.depth;
    p.noise = Range(0, 50);
    p.life = Range(0, 100);   // per-leaf seed for its turbulence
    p.alpha = 1;
    p.flip = 1;
    PickFlight(p);
    return p;
}

Particle Sim::NewRain(float wind)
{
    Particle p = {};
    p.kind = KRain;
    p.x = SpawnX(wind * 0.4f);
    const float S = ScaleAt(p.x);
    p.depth = Range(0.5f, 1.0f);
    p.y = -Range(20.0f, 120.0f) * S;
    p.vx = wind * 1.3f;
    p.vy = 1300.0f * S * p.depth;
    p.alpha = 1;
    return p;
}

Particle Sim::NewSnow(float wind)
{
    Particle p = {};
    p.kind = KSnow;
    p.x = SpawnX(wind);
    const float S = ScaleAt(p.x);
    // near flakes are bigger and quicker, far ones small, slow and faint; now and then a heavier clump
    p.depth = Range(0.35f, 1.0f);
    bool clump = Rand() < 0.06f;
    p.size = Range(2.5f, 5.5f) * S * p.depth * (clump ? 1.8f : 1.0f);
    p.fall = Range(32.0f, 60.0f) * S * (0.5f + 0.5f * p.depth) * (clump ? 1.3f : 1.0f);
    p.amp = Range(12.0f, 35.0f) * S * p.depth;
    p.y = -10.0f * S;
    p.vx = wind * 0.75f;
    p.vy = p.fall;
    p.phase = Range(0, 2 * Pi);
    p.phase2 = Range(0, 2 * Pi);
    p.freq = Range(0.3f, 0.9f);
    p.freq2 = Range(0.9f, 1.8f);
    p.life = Range(0, 100);
    p.alpha = 1;
    return p;
}

void Sim::Seed(int kind, int count, float wind, const Tracker& tr)
{
    for (int i = 0; i < count && parts.size() < MaxParticles; i++)
    {
        Particle p = kind == KLeaf ? NewLeaf(wind) : kind == KRain ? NewRain(wind) : NewSnow(wind);
        p.x = Range(0.0f, float(W));
        p.y = Range(0.0f, float(H));
        if (p.y > GroundAt(p.x) - 2 || Swallowed(p.x, p.y, tr)) continue;
        parts.push_back(p);
    }
}

void Sim::Splash(float x, float y, float depth, int tint, int count, float power)
{
    if (parts.size() + pending.size() > MaxParticles - 8) return;
    int n = count > 0 ? count : 2 + int(Rand() * 3);
    for (int i = 0; i < n; i++)
    {
        Particle p = {};
        p.kind = KSplash;
        p.color = uint8_t(tint);
        p.x = x;
        p.y = y - 1;
        p.vx = Range(-90.0f, 90.0f) * S * power;
        p.vy = -Range(100.0f, 300.0f) * S * depth * power;
        p.life = Range(0.2f, 0.45f);
        p.size = Range(1.2f, 2.5f) * S * depth * (tint ? 1.3f : 1.0f);
        p.depth = depth;
        p.alpha = 1;
        pending.push_back(p);
    }
}

void Sim::Ripple(float x, float y, float depth)
{
    if (parts.size() + pending.size() > MaxParticles - 8) return;
    Particle p = {};
    p.kind = KRipple;
    p.x = x;
    p.y = y;
    p.depth = depth;
    p.life = Range(0.5f, 0.8f);
    p.alpha = 1;
    pending.push_back(p);
}

// A drop leaving a window's top corner: either it runs down the window's side (mode 2) and lets go at the
// bottom, or it falls straight off the corner like from a gutter (mode 1, accelerating under gravity).
void Sim::Drip(const Win& w, int side)
{
    if (parts.size() + pending.size() > MaxParticles - 8) return;
    Particle p = {};
    p.kind = KRain;
    p.depth = Range(0.6f, 0.9f);
    p.host = w.h;
    p.offx = float(side);
    p.x = side < 0 ? w.l - 1.5f * S : w.r + 1.5f * S;
    p.y = w.t + 2;
    p.alpha = 1;
    p.phase = Range(0, 6.28f);
    if (Rand() < 0.5f)
    {
        p.mode = 2;
        p.fall = Range(90.0f, 240.0f) * S;   // how fast it runs down the side
        p.rest = 0;                           // distance travelled down from the top corner
    }
    else
    {
        p.mode = 1;
        p.vy = Range(20.0f, 80.0f) * S;
    }
    pending.push_back(p);
}

// ---------------------------------------------------------------- stepping

void Sim::Step(float dt, const Tracker& tr, const EffectMix& m, const CursorWind& cursor)
{
    time += dt;
    stepDt = dt;
    mix = m;
    float wind = Wind();

    resting = 0;
    for (const Particle& p : parts)
        if (p.state == Resting) resting++;

    UpdateCaps(dt, tr);

    float sx = W / 1920.0f;
    if (m.leaves > 0 && prevMix.leaves <= 0) Seed(KLeaf, int(m.leaves * LeafRate * sx * H / (55.0f * S)), wind, tr);
    if (m.rain > 0 && prevMix.rain <= 0) Seed(KRain, int(m.rain * RainRate * sx * H / (1000.0f * S)), wind, tr);
    if (m.snow > 0 && prevMix.snow <= 0) Seed(KSnow, int(m.snow * SnowRate * sx * H / (45.0f * S)), wind, tr);
    prevMix = m;

    leafAcc += m.leaves * LeafRate * sx * dt;
    rainAcc += m.rain * RainRate * sx * dt;
    snowAcc += m.snow * SnowRate * sx * dt;
    for (; leafAcc >= 1; leafAcc -= 1) if (parts.size() < MaxParticles) parts.push_back(NewLeaf(wind));
    for (; rainAcc >= 1; rainAcc -= 1) if (parts.size() < MaxParticles) parts.push_back(NewRain(wind));
    for (; snowAcc >= 1; snowAcc -= 1) if (parts.size() < MaxParticles) parts.push_back(NewSnow(wind));

    if (cursor.valid) this->cursor = cursor;
    tracker = &tr;
    PROFILE_START(p1);
    ApplyCursor(cursor, dt, tr);
    PROFILE(1, p1);
    PROFILE_START(p2);
    ApplyGusts(cursor, dt);
    PROFILE(2, p2);
    PROFILE_START(p3);

    for (size_t i = parts.size(); i-- > 0;)
    {
        if (!Update(parts[i], dt, wind, tr))
        {
            parts[i] = parts.back();
            parts.pop_back();
        }
    }
    parts.insert(parts.end(), pending.begin(), pending.end());
    pending.clear();
    PROFILE(3, p3);
    PROFILE_START(p4);
    StackLeaves(tr);
    PROFILE(4, p4);
    PROFILE_START(p5);
    UpdateCritters(dt, tr);
    PROFILE(5, p5);

    flash = std::max(0.0f, flash - dt * 4.0f);
    if (m.thunder && Rand() < dt * 0.05f) flash = 1.0f;
}

bool Sim::Fast(const Win& w, float dt) const
{
    return std::fabs(w.dx) + std::fabs(w.dy) > 800.0f * S * dt;
}

void Sim::UpdateCaps(float dt, const Tracker& tr)
{
    float melt = mix.snow > 0 ? 0.04f * S : (mix.tempC > 2 ? 1.2f * S : 0.35f * S);
    bool raining = mix.rain > 0;
    float evaporate = raining ? 0.002f * S : 0.06f * S;
    float dry = raining ? 0.0f : 0.012f * S;
    float spillL, spillR;

    for (size_t i = caps.size(); i-- > 0;)
    {
        SnowCap& c = caps[i];
        const Win* w = tr.Get(c.h);
        bool keep = true;
        if (!w || !w->surface)
        {
            keep = time - c.lastSeen < 2.0f;
        }
        else
        {
            c.lastSeen = time;
            c.Resize(int(w->r - w->l));
            if (Fast(*w, dt))
            {
                // a shaken window throws its snow and water off
                for (size_t j = 0; j < c.height.size(); j += 2)
                {
                    float h = c.height[j], wa = c.water[j];
                    float x = w->l + j * SnowCap::Bucket + Rand() * SnowCap::Bucket;
                    int n = h >= 1.5f ? 1 + int(h / (6.0f * S)) : 0;
                    for (int k = 0; k < n && parts.size() + pending.size() < MaxParticles; k++)
                    {
                        Particle p = NewSnow(0);
                        p.x = x;
                        p.y = w->t - Rand() * h - 2;
                        p.vx = w->dx / dt * 0.4f + Range(-60, 60) * S;
                        p.vy = -Range(0, 150) * S;
                        pending.push_back(p);
                    }
                    if (wa > 0.5f * S && Rand() < 0.3f) Splash(x, w->t - wa, 1.0f, 0, 2, 1.5f);
                    c.height[j] *= 0.25f;
                    if (j + 1 < c.height.size()) c.height[j + 1] *= 0.25f;
                }
                for (float& wa : c.water) wa *= 0.3f;
            }
            c.Relax(dt, melt);
            SettleSnow(c, w->l, false, dt);
            SlideSnowOffEdges(c, *w, dt);
            c.Flow(dt, evaporate, dry, spillL, spillR);
            // water reaching a corner gathers into a bead that lets go once it is heavy enough;
            // in a downpour that becomes a steady trickle like from a gutter
            const float drop = 0.5f * S;
            c.beadL += spillL;
            c.beadR += spillR;
            for (int k = 0; k < 4 && c.beadL > drop; k++) { c.beadL -= drop; Drip(*w, -1); }
            for (int k = 0; k < 4 && c.beadR > drop; k++) { c.beadR -= drop; Drip(*w, 1); }
            c.beadL = std::min(c.beadL, drop);
            c.beadR = std::min(c.beadR, drop);
            bool heap = false;
            for (float h : c.pile) heap |= h > 0.3f;
            keep = c.Any() || heap || mix.snow > 0 || raining;
        }
        if (!keep)
        {
            caps[i] = std::move(caps.back());
            caps.pop_back();
        }
    }
    ground.Relax(dt, melt);
    SettleSnow(ground, 0, true, dt);
    ground.Flow(dt, evaporate, dry, spillL, spillR);
}

bool Sim::FindLanding(float x, float y0, float y1, const Tracker& tr, const Win*& hit, float& hy) const
{
    for (const Win& w : tr.all)
    {
        if (!w.surface || x < w.l || x > w.r) continue;
        const SnowCap* cap = CapOf(w.h);
        float top = w.t - (cap ? cap->At(x - w.l) + HeapAt(*cap, x - w.l) : 0.0f);
        if (y0 <= top && y1 >= top && !tr.Covered(x, top - 2, w.z))
        {
            hit = &w;
            hy = top;
            return true;
        }
    }
    return false;
}

bool Sim::Swallowed(float x, float y, const Tracker& tr) const
{
    for (const Win& w : tr.all)
    {
        if (w.surface && x > w.l && x < w.r && y > w.t + 4 && y < w.b && !tr.Covered(x, y, w.z)) return true;
    }
    return false;
}

void Sim::UpdateLeafFlight(Particle& p, float dt, float wind)
{
    p.modeTime -= dt;
    if (p.modeTime <= 0) PickFlight(p);   // leaves change their way of falling now and then
    p.phase += p.freq * dt;
    p.phase2 += p.freq2 * dt;
    p.noise += dt;

    // per-leaf turbulence so no two leaves follow the same curve
    float turb = 0.6f * std::sin(p.noise * 1.3f + p.life) + 0.4f * std::sin(p.noise * 2.9f + p.life * 2.1f);
    float s = std::sin(p.phase), c = std::cos(p.phase);
    float tvx, tvy;
    p.spin += p.rotv * dt;
    switch (p.mode)
    {
    case Pendulum:
        tvx = wind * 0.9f + p.amp * c * (1 + 0.3f * turb);
        tvy = p.fall * (0.2f + 1.25f * c * c) + turb * 12.0f * S;   // fastest mid-swing, stalls at the ends
        p.rot = p.spin + p.tilt * c;
        p.flip = std::cos(p.phase2) * (0.75f + 0.25f * std::fabs(c));
        break;
    case Tumble:
        tvx = wind + p.amp * s + turb * 30.0f * S;
        tvy = p.fall * 1.15f + turb * 15.0f * S;
        p.rot = p.spin;
        p.flip = std::cos(p.phase2);
        break;
    default:
        tvx = wind * 0.85f + p.amp * c;
        tvy = p.fall * 0.95f + turb * 10.0f * S;
        p.rot = p.spin + p.tilt * s;
        p.flip = s;
        break;
    }
    // after a gust a leaf coasts on its own momentum for a while before its usual flight takes over again
    float coast = p.rest > 0 ? 0.2f : 1.0f;
    p.rest = std::max(0.0f, p.rest - dt);
    p.vx += (tvx - p.vx) * std::min(1.0f, dt * p.drag * 2.5f * coast);
    p.vy += (tvy - p.vy) * std::min(1.0f, dt * p.drag * 3.0f * coast);
}

bool Sim::Update(Particle& p, float dt, float wind, const Tracker& tr)
{
    p.age += dt;
    if (p.kind == KRipple) return p.age < p.life;
    if (p.state != Falling) return UpdateResting(p, dt, tr);

    switch (p.kind)
    {
    case KLeaf:
        UpdateLeafFlight(p, dt, wind);
        break;
    case KRain:
        if (p.mode == 2)
        {
            // running down a window's side edge, wobbling a little, until it reaches the bottom corner
            const Win* w = tr.Get(p.host);
            if (!w || !w->surface) { p.mode = 1; break; }
            p.rest += p.fall * (0.8f + 0.4f * std::sin(p.age * 3.0f + p.phase)) * dt;
            float h = w->b - w->t;
            p.x = (p.offx < 0 ? w->l - 1.5f * S : w->r + 1.5f * S) + std::sin(p.age * 5.0f + p.phase) * 0.6f * S;
            p.y = w->t + std::min(p.rest, h);
            p.hidden = tr.Covered(p.x, p.y, w->z);
            if (p.rest >= h)
            {
                p.mode = 1;
                p.vx = 0;
                p.vy = p.fall * 0.5f;
                p.hidden = false;
            }
            return true;
        }
        if (p.mode == 1)
        {
            // dripping: accelerates from rest instead of arriving at full speed
            p.vy = std::min(1300.0f * S, p.vy + 2200.0f * S * dt);
            p.vx += (wind * 0.3f - p.vx) * std::min(1.0f, dt * 2);
            break;
        }
        p.vx += (wind * 1.3f - p.vx) * std::min(1.0f, dt * 4);
        break;
    case KSnow:
    {
        // a lazy meander from two slow oscillators per flake; far flakes feel less wind
        p.phase += p.freq * dt;
        p.phase2 += p.freq2 * dt;
        float drift = std::sin(p.phase) * p.amp + std::sin(p.phase2 + p.life) * p.amp * 0.35f;
        float k = std::min(1.0f, dt * 1.5f);
        p.vx += (wind * (0.55f + 0.35f * p.depth) + drift - p.vx) * k;
        p.vy += (p.fall * (1.0f + 0.15f * std::sin(p.phase2)) - p.vy) * k;
        break;
    }
    case KSplash:
        p.vy += 1600.0f * S * dt;
        p.x += p.vx * dt;
        p.y += p.vy * dt;
        return p.age < p.life;
    }

    float py = p.y;
    p.x += p.vx * dt;
    p.y += p.vy * dt;
    if (p.y > H + 60 * S || p.x < -400 * S || p.x > W + 400 * S) return false;
    if (p.vy <= 0) return true;

    // a leaf touches down with its lower edge, not its middle
    float reach = p.kind == KLeaf ? p.size * 0.22f : 0.0f;
    const Win* hit = nullptr;
    float hy = 0;
    if (FindLanding(p.x, py + reach, p.y + reach, tr, hit, hy)) return Land(p, hit, hy);
    if (p.x >= 0 && p.x < W)
    {
        float gy = GroundAt(p.x) - ground.At(p.x) - HeapAt(ground, p.x);
        if (py + reach <= gy && p.y + reach >= gy) return Land(p, nullptr, gy);
        if (p.y > gy + 20 * S) return false;
    }
    return !Swallowed(p.x, p.y, tr);
}

bool Sim::Land(Particle& p, const Win* w, float y)
{
    switch (p.kind)
    {
    case KRain:
    {
        SnowCap& c = w ? CapFor(*w) : ground;
        float rel = w ? p.x - w->l : p.x;
        int i = std::max(0, std::min(int(c.water.size()) - 1, int(rel / SnowCap::Bucket)));
        bool wet = c.water[i] > 0.4f * S;
        if (wet)
        {
            // a drop into a puddle: a ring, a little crown and a dent that travels as a wave
            c.waveV[i] -= 25.0f * S * p.depth;
            if (Rand() < 0.35f) Ripple(p.x, y, p.depth);
            Splash(p.x, y, p.depth, 0, 1 + (Rand() < 0.5f ? 1 : 0), 0.6f);
        }
        else Splash(p.x, y, p.depth, !w && c.mud[i] > 1.5f * S && Rand() < 0.3f ? 1 : 0);
        c.AddWater(rel, 0.025f * S * p.depth * (w ? 1.0f : 2.0f));
        if (!w) c.AddMud(rel, 0.006f * S * (wet ? 2.0f : 1.0f));
        return false;
    }
    case KSnow:
        if (w) CapFor(*w).Add(p.x - w->l, p.size * 0.55f);
        else ground.Add(p.x, p.size * 0.55f);
        return false;
    default:
    {
        // keep a little of the sideways drift so it skids to a stop, and settle from the falling pose
        float ls = ScaleAt(p.x);
        p.vx = std::max(-20.0f * ls, std::min(20.0f * ls, p.vx * 0.1f));
        p.vy = 0;
        p.y = y;
        p.onGround = w == nullptr;
        p.host = w ? w->h : nullptr;
        p.offx = w ? p.x - w->l : 0;
        p.startRot = p.rot;
        // mirrored or not makes no difference to a leaf; never squash through zero while settling
        p.startFlip = std::max(0.2f, std::fabs(p.flip));
        // lying with its length along the ground: whichever way round is nearer to how it came down
        float r = std::fmod(p.rot, 360.0f);
        if (r < 0) r += 360;
        float to90 = std::fabs(std::fmod(r - 90 + 540.0f, 360.0f) - 180), to270 = std::fabs(std::fmod(r - 270 + 540.0f, 360.0f) - 180);
        p.restRot = (to90 < to270 ? 90.0f : 270.0f) + Range(-12, 12);
        p.rot = p.restRot;
        p.settle = 0;
        p.heaped = false;
        // landing on a heap the user swept up: it joins it, on top
        SnowCap& cap = w ? CapFor(*w) : ground;
        float rel = p.x - (w ? w->l : 0);
        if (HeapAt(cap, rel) > 0.5f * ls) JoinHeap(p, cap, w ? w->l : 0, 1.0f, false);
        p.alpha = 1;
        if (resting >= MaxResting) p.state = Fading;
        else { p.state = Resting; p.rest = Range(40, 120); resting++; }
        p.landed = time;
        p.landSeq = ++landCounter;
        return true;
    }
    }
}

bool Sim::UpdateResting(Particle& p, float dt, const Tracker& tr)
{
    p.settle = std::min(1.0f, p.settle + dt / 0.35f);
    if (p.onGround)
    {
        // a nudge (from a hedgehog, say) slides it along smoothly instead of teleporting it
        p.x = std::max(0.0f, std::min(float(W) - 1, p.x + p.vx * dt));
        p.vx *= std::exp(-dt * 8);
        if (!p.heaped) p.y = GroundAt(p.x) - ground.At(p.x);   // heaped leaves are placed by StackLeaves
        p.hidden = tr.Covered(p.x, p.y - 2, INT32_MAX, true);
    }
    else
    {
        const Win* w = tr.Get(p.host);
        bool fall = !w || !w->surface || p.offx < 0 || p.offx > w->r - w->l;
        if (!fall && Fast(*w, dt))
        {
            // flung off a window that is being dragged around
            p.state = Falling;
            p.vx = std::max(-1500.0f * S, std::min(1500.0f * S, w->dx / dt * 0.5f));
            p.vy = std::min(-80.0f * S, w->dy / dt * 0.5f) - Range(0, 150) * S;
            p.rotv = Range(-400, 400);
            p.modeTime = Range(0.6f, 1.5f);
            p.spin = p.rot;
            p.heaped = false;
            p.y -= 3;
            p.hidden = false;
            return true;
        }
        if (fall)
        {
            if (p.state == Fading) return false;
            p.state = Falling;
            p.vx = p.vy = 0;
            p.spin = p.rot;
            p.heaped = false;
            p.hidden = false;
            return true;
        }
        p.offx += p.vx * dt;
        p.vx *= std::exp(-dt * 8);
        p.x = w->l + p.offx;
        if (!p.heaped) p.y = w->t - CapAt(*w, p.x);
        p.hidden = tr.Covered(p.x, p.y - 2, w->z);
    }

    if (p.state == Resting)
    {
        p.rest -= dt;
        if (p.rest <= 0) p.state = Fading;
        return true;
    }
    p.alpha -= dt * 0.5f;
    return p.alpha > 0;
}

// ---------------------------------------------------------------- mouse wind

float Sim::CursorFalloff(const CursorWind& c, float x, float y, float radius) const
{
    // distance to the segment the pointer travelled this frame, so fast swipes act along their whole path
    if (x < std::min(c.x0, c.x1) - radius || x > std::max(c.x0, c.x1) + radius ||
        y < std::min(c.y0, c.y1) - radius || y > std::max(c.y0, c.y1) + radius) return 0;
    float dx = c.x1 - c.x0, dy = c.y1 - c.y0;
    float len2 = dx * dx + dy * dy;
    float t = len2 > 0 ? std::max(0.0f, std::min(1.0f, ((x - c.x0) * dx + (y - c.y0) * dy) / len2)) : 0.0f;
    float px = c.x0 + t * dx - x, py = c.y0 + t * dy - y;
    float d2 = px * px + py * py;
    if (d2 >= radius * radius) return 0;
    float f = 1 - std::sqrt(d2) / radius;
    return f * f;
}

void Sim::BlowCap(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt)
{
    const float R = 150.0f * S;
    for (size_t j = 0; j < c.height.size(); j++)
    {
        float h = c.height[j];
        if (h < 0.5f) continue;
        float x = left + j * SnowCap::Bucket + SnowCap::Bucket / 2.0f;
        float base = (w ? w->t : groundY[std::min(j, groundY.size() - 1)]) - c.mud[j];
        float f = CursorFalloff(cursor, x, base - h * 0.5f, R);
        if (f <= 0) continue;
        float take = h * std::min(1.0f, f * dt * 4.0f);
        c.height[j] -= take;
        float n = take / (2.0f * S);
        int count = int(n) + (Rand() < n - int(n) ? 1 : 0);
        for (int k = 0; k < count && parts.size() < MaxParticles; k++)
        {
            Particle p = NewSnow(0);
            p.x = x + Range(-2.0f, 2.0f) * S;
            p.y = base - Rand() * h - 1;
            p.vx = cursor.vx * 0.6f * f + Range(-60, 60) * S;
            p.vy = cursor.vy * 0.6f * f - Range(40, 200) * S;
            parts.push_back(p);
        }
    }
}

void Sim::StirWater(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt)
{
    const float R = 160.0f * S;
    float speed = std::sqrt(cursor.vx * cursor.vx + cursor.vy * cursor.vy);
    int n = int(c.water.size());
    int dir = cursor.vx > 0 ? 1 : -1;
    for (int j = 0; j < n; j++)
    {
        if (c.water[j] < 0.3f * S) continue;
        float x = left + j * SnowCap::Bucket + SnowCap::Bucket / 2.0f;
        float base = w ? w->t : groundY[std::min(size_t(j), groundY.size() - 1)];
        float y = base - c.mud[j] - c.height[j] - c.water[j];
        float f = CursorFalloff(cursor, x, y, R);
        if (f <= 0) continue;

        // the pointer drags a bow wave along and sloshes water in its direction
        c.waveV[j] += (cursor.vy * 0.025f - std::fabs(cursor.vx) * 0.01f) * f;
        int to = j + dir;
        if (to >= 0 && to < n)
        {
            float mv = c.water[j] * std::min(1.0f, f * dt * std::fabs(cursor.vx) / (200.0f * S)) * 0.5f;
            c.water[j] -= mv;
            c.water[to] = std::min(c.waterMax, c.water[to] + mv);
        }
        // fast swipes throw droplets, muddy ones where the puddle sits on mud
        if (speed > 500.0f * S && Rand() < f * dt * speed / (120.0f * S) && parts.size() < MaxParticles)
        {
            Particle p = {};
            p.kind = KSplash;
            p.color = c.mud[j] > 1.0f * S && Rand() < 0.5f ? 1 : 0;
            p.x = x;
            p.y = y - 1;
            p.vx = cursor.vx * 0.35f * f + Range(-60, 60) * S;
            p.vy = -Range(150, 400) * S * f - std::fabs(cursor.vy) * 0.2f;
            p.life = Range(0.35f, 0.7f);
            p.size = Range(1.5f, 3.0f) * S;
            p.depth = 1;
            p.alpha = 1;
            parts.push_back(p);
        }
    }
}

void Sim::ApplyCursor(const CursorWind& c, float dt, const Tracker& tr)
{
    if (!c.valid) return;
    float speed = std::sqrt(c.vx * c.vx + c.vy * c.vy);
    if (speed < 40.0f * S) return;
    const float R = 190.0f * S;
    float k = std::min(1.0f, dt * 10.0f);
    bool strong = speed > 450.0f * S;

    for (Particle& p : parts)
    {
        if (p.kind == KSplash || p.kind == KRipple) continue;
        float f = CursorFalloff(c, p.x, p.y, R);
        if (f <= 0) continue;
        if (p.state != Falling)
        {
            if (p.kind == KLeaf && !(strong && f > 0.55f && Rand() < 0.2f))
            {
                // the pointer sweeps resting leaves along the edge: a swept leaf becomes part of a heap,
                // whose volume moves with the sweep (TransportHeap) and slumps into a mound
                if (!p.heaped)
                {
                    const Win* w = p.onGround ? nullptr : tr.Get(p.host);
                    if (p.onGround) JoinHeap(p, ground, 0, Range(0.05f, 1.0f), true);
                    else if (w) JoinHeap(p, CapFor(*w), w->l, Range(0.05f, 1.0f), true);
                }
                // its position now follows the heap's mass as TransportHeap moves it
                p.rest = std::max(p.rest, 60.0f);
                continue;
            }
            if (strong && f > 0.1f)
            {
                // a hard swipe right over them lifts leaves back into the air
                p.state = Falling;
                p.vx = c.vx * 0.35f * f;
                p.vy = c.vy * 0.35f * f - Range(60, 150) * S;
                p.rotv = Range(-500, 500);
                p.modeTime = Range(0.6f, 1.5f);
                p.rest = 1.0f;   // coasts on the gust for a moment
                p.spin = p.rot;
                p.heaped = false;
                p.y -= 3;
                p.hidden = false;
            }
            continue;
        }
        // falling things are moved by the gusts the pointer sheds (ApplyGusts), not dragged along with it
    }

    for (SnowCap& cap : caps)
    {
        const Win* w = tr.Get(cap.h);
        if (!w || !w->surface) continue;
        TransportHeap(cap, w->l, w, c, dt);
        if (strong) BlowCap(cap, w->l, w, c, dt);
        StirWater(cap, w->l, w, c, dt);
    }
    TransportHeap(ground, 0, nullptr, c, dt);
    if (strong) BlowCap(ground, 0, nullptr, c, dt);
    StirWater(ground, 0, nullptr, c, dt);
}

// ---------------------------------------------------------------- leaf heaps

// A heap is a persistent height profile of leaf volume along a surface, like a snow drift. Sweeping with the
// pointer moves that volume along (TransportHeap); every frame the slopes slump back to a natural angle. The
// leaves in the heap sit on its surface at a fixed depth each, so they follow the heap smoothly instead of
// restacking themselves frame by frame.
float Sim::HeapAt(const SnowCap& c, float rel) const
{
    int n = int(c.pile.size());
    if (n == 0) return 0;
    float u = rel / SnowCap::Bucket - 0.5f;
    int i = int(std::floor(u));
    float t = u - i;
    int i0 = std::max(0, std::min(n - 1, i)), i1 = std::max(0, std::min(n - 1, i + 1));
    return c.pile[i0] + (c.pile[i1] - c.pile[i0]) * t;
}

void Sim::JoinHeap(Particle& p, SnowCap& c, float left, float depth, bool addVolume)
{
    int n = int(c.pile.size());
    if (n == 0) return;
    // swept leaves build the heap; a leaf that merely lands on one lies on top without reshaping it
    float vol = addVolume ? p.size * p.size * 0.06f : 0.0f;
    float rel = p.x - left, half = p.size * 0.4f;
    int a = std::max(0, int((rel - half) / SnowCap::Bucket)), b = std::min(n - 1, int((rel + half) / SnowCap::Bucket));
    if (b < a) b = a;
    float h = vol / ((b - a + 1) * float(SnowCap::Bucket));
    for (int j = a; j <= b; j++) c.pile[j] += h;
    p.heaped = true;
    p.vy = depth;
    p.heapVol = p.heapVol0 = vol;
    // remember its place in the heap's mass, so it moves along when the heap is swept or slumps
    float total = 0, before = 0, B = float(SnowCap::Bucket);
    for (int j = 0; j < n; j++)
    {
        total += c.pile[j];
        if ((j + 1) * B <= rel) before += c.pile[j];
        else if (j * B < rel) before += c.pile[j] * (rel - j * B) / B;
    }
    p.massFrac = total > 0 ? before / total : 0.5f;
}

void Sim::TransportHeap(SnowCap& c, float left, const Win* w, const CursorWind& cursor, float dt)
{
    int n = int(c.pile.size());
    if (n < 2 || std::fabs(cursor.vx) < 1) return;
    const float R = 190.0f * S, B = float(SnowCap::Bucket);
    int dir = cursor.vx > 0 ? 1 : -1;
    heapTmp = c.pile;
    bool moved = false;
    float lo = 1e9f, hi = -1e9f;
    for (int j = 0; j < n; j++)
    {
        if (c.pile[j] < 0.05f) continue;
        float base = w ? w->t : groundY[std::min(size_t(j), groundY.size() - 1)];
        float f = CursorFalloff(cursor, left + (j + 0.5f) * B, base - c.pile[j] * 0.5f, R);
        if (f <= 0) continue;
        int to = j + dir;
        if (to < 0 || to >= n) continue;
        // the same push the swept leaves get, as a fraction of a bucket's volume per frame
        float shift = std::min(12.0f * S, std::fabs(cursor.vx) * f * dt * 0.35f);
        float m = c.pile[j] * std::min(0.8f, shift / B);
        heapTmp[j] -= m;
        heapTmp[to] += m;
        moved = true;
        lo = std::min(lo, j * B);
        hi = std::max(hi, (to + 1) * B);
    }
    if (moved)
    {
        c.pile.swap(heapTmp);
        // leaves in the swept stretch follow the moving heap for a moment
        if (c.stirred > 0) { c.stirL = std::min(c.stirL, lo); c.stirR = std::max(c.stirR, hi); }
        else { c.stirL = lo; c.stirR = hi; }
        c.stirred = 0.6f;
    }
}

void Sim::StackLeaves(const Tracker& tr)
{
    const float dt = stepDt, B = float(SnowCap::Bucket);

    // how much volume each surface's heaped leaves account for; fading leaves give theirs up gradually
    float needGround = 0;
    std::vector<float> need(caps.size(), 0.0f);
    for (Particle& p : parts)
    {
        if (p.kind != KLeaf || p.state == Falling || !p.heaped) continue;
        if (p.state == Fading) p.heapVol = std::max(0.0f, p.heapVol - p.heapVol0 * 0.5f * dt);
        if (p.onGround) needGround += p.heapVol;
        else
            for (size_t i = 0; i < caps.size(); i++)
                if (caps[i].h == p.host) { need[i] += p.heapVol; break; }
    }

    auto settle = [&](SnowCap& c, float required) {
        int n = int(c.pile.size());
        float actual = 0;
        for (float h : c.pile) actual += h * B;
        if (actual <= 0.01f) return;
        // leaves that left the heap (lifted, eaten, faded) take their share with them, smoothly
        if (actual > required + 1)
        {
            float k = 1 - std::min(1.0f, dt * 3) * (1 - required / actual);
            for (float& h : c.pile) h *= k;
        }
        // slopes slump until they are no steeper than about 35 degrees
        const float lim = 0.7f * B;
        for (int pass = 0; pass < 2; pass++)
            for (int j = 0; j + 1 < n; j++)
            {
                float d = c.pile[j] - c.pile[j + 1];
                if (d > lim) { float t = (d - lim) * 0.3f; c.pile[j] -= t; c.pile[j + 1] += t; }
                else if (d < -lim) { float t = (-d - lim) * 0.3f; c.pile[j] += t; c.pile[j + 1] -= t; }
            }
        for (float& h : c.pile) if (h < 0.02f) h = 0;
    };
    settle(ground, needGround);
    for (size_t i = 0; i < caps.size(); i++) settle(caps[i], need[i]);

    // While the pointer is sweeping a heap, its leaves keep their place in the heap's mass and glide to
    // where that mass is now (inverse of the cumulative volume), so they travel with the sweep. The rest of
    // the time they stay put and their place in the mass is just re-read from where they lie.
    auto cumulative = [&](const SnowCap& c, std::vector<float>& cum) {
        cum.resize(c.pile.size() + 1);
        cum[0] = 0;
        for (size_t j = 0; j < c.pile.size(); j++) cum[j + 1] = cum[j] + c.pile[j];
        return cum.back();
    };
    auto massToRel = [&](const std::vector<float>& cum, float frac) {
        float target = frac * cum.back();
        size_t j = size_t(std::upper_bound(cum.begin() + 1, cum.end(), target) - cum.begin());
        if (j >= cum.size()) return (cum.size() - 1) * B;
        float span = cum[j] - cum[j - 1];
        return (j - 1 + (span > 0 ? (target - cum[j - 1]) / span : 0.5f)) * B;
    };
    auto relToMass = [&](const std::vector<float>& cum, float rel) {
        float u = std::max(0.0f, std::min(float(cum.size() - 1), rel / B));
        size_t j = std::min(cum.size() - 2, size_t(u));
        float v = cum[j] + (cum[j + 1] - cum[j]) * (u - j);
        return cum.back() > 0 ? v / cum.back() : 0.5f;
    };
    ground.stirred = std::max(0.0f, ground.stirred - dt);
    for (SnowCap& c : caps) c.stirred = std::max(0.0f, c.stirred - dt);
    std::vector<float>& cumGround = heapTmp;
    bool groundHeap = cumulative(ground, cumGround) > 0;
    std::vector<float> cumCap;
    float glide = std::min(1.0f, dt * 8), ease = std::min(1.0f, dt * 10);
    for (Particle& p : parts)
    {
        if (p.kind != KLeaf || p.state == Falling || !p.heaped) continue;
        const SnowCap* c;
        const std::vector<float>* cum;
        float left, base;
        if (p.onGround)
        {
            if (!groundHeap) continue;
            c = &ground;
            cum = &cumGround;
            left = 0;
            base = GroundAt(p.x) - ground.At(p.x);
        }
        else
        {
            const Win* w = tr.Get(p.host);
            c = w ? CapOf(w->h) : nullptr;
            if (!c || cumulative(*c, cumCap) <= 0) continue;
            cum = &cumCap;
            left = w->l;
            base = w->t - c->At(p.x - left);
        }
        float rel = p.x - left;
        float margin = 40.0f * ScaleAt(p.x);
        float goal = c->stirred > 0 && rel > c->stirL - margin && rel < c->stirR + margin ? massToRel(*cum, p.massFrac) : rel;
        // only short moves along with the swept mass; never jump to some other heap further away
        if (std::fabs(goal - rel) < 2 * margin && goal != rel)
        {
            rel += (goal - rel) * glide;
            if (p.onGround) p.x = rel;
            else { p.offx = rel; p.x = left + rel; }
        }
        else p.massFrac = relToMass(*cum, rel);
        // ride on the heap surface at its own depth, easing so small reshapes don't make it bob
        float target = base - HeapAt(*c, rel) * p.vy;
        p.y = p.settle < 1 ? target : p.y + (target - p.y) * ease;
    }

    restOrder.clear();
    for (size_t i = 0; i < parts.size(); i++)
        if (parts[i].kind == KLeaf && parts[i].state != Falling) restOrder.push_back(int(i));
    // draw flat leaves first in landing order, then heaped ones from the inside out
    std::sort(restOrder.begin(), restOrder.end(), [&](int a, int b) {
        const Particle& pa = parts[a];
        const Particle& pb = parts[b];
        if (pa.heaped != pb.heaped) return !pa.heaped;
        if (pa.heaped && pa.vy != pb.vy) return pa.vy < pb.vy;
        return pa.landSeq < pb.landSeq;
    });
}
// ---------------------------------------------------------------- pointer gusts

// A moving pointer sheds puffs of air. Each puff carries on in the pointer's direction, slows, widens and
// fades within about half a second. Whatever it passes through gets a push along it, a nudge sideways out of
// its path and a little curl, then keeps its own momentum: like waving a hand through falling leaves.
void Sim::ApplyGusts(const CursorWind& c, float dt)
{
    gustClock += dt;
    if (c.valid)
    {
        float speed = std::sqrt(c.vx * c.vx + c.vy * c.vy);
        if (speed > 150.0f * S && gustClock >= 1.0f / 40 && gusts.size() < 24)
        {
            gustClock = 0;
            float k = std::min(1.0f, 2000.0f * S / speed) * 0.4f;
            Gust g;
            g.x = c.x1;
            g.y = c.y1;
            g.vx = c.vx * k;
            g.vy = c.vy * k;
            g.r = std::max(50.0f * S, std::min(140.0f * S, 40.0f * S + speed * 0.025f));
            g.age = 0;
            g.life = 0.55f + std::min(0.5f, speed / (4000.0f * S));
            g.swirl = Rand() < 0.5f ? -1.0f : 1.0f;
            gusts.push_back(g);
        }
    }
    if (gusts.empty()) return;

    float drag = std::exp(-dt * 2.2f);
    for (size_t i = gusts.size(); i-- > 0;)
    {
        Gust& g = gusts[i];
        g.age += dt;
        if (g.age >= g.life) { gusts[i] = gusts.back(); gusts.pop_back(); continue; }
        g.x += g.vx * dt;
        g.y += g.vy * dt;
        g.vx *= drag;
        g.vy *= drag;
        g.r += 90.0f * S * dt;
    }

    for (Particle& p : parts)
    {
        if (p.state != Falling || p.kind == KSplash || p.kind == KRipple || (p.kind == KRain && p.mode == 2)) continue;
        for (const Gust& g : gusts)
        {
            float dx = p.x - g.x, dy = p.y - g.y;
            if (std::fabs(dx) > g.r || std::fabs(dy) > g.r) continue;
            float d2 = dx * dx + dy * dy;
            if (d2 >= g.r * g.r) continue;
            float d = std::sqrt(d2) + 1e-3f;
            float fade = 1 - g.age / g.life;
            float f = (1 - d / g.r) * (1 - d / g.r) * fade * fade;
            float weight = p.kind == KRain ? 0.15f : p.kind == KSnow ? 1.3f : 1.0f;
            float a = f * weight * 3.0f * dt;
            float gs = std::sqrt(g.vx * g.vx + g.vy * g.vy);
            float nx = dx / d, ny = dy / d;
            float ax = (g.vx - p.vx * 0.3f) + nx * gs * 0.2f - ny * gs * 0.12f * g.swirl;
            float ay = (g.vy - p.vy * 0.3f) + ny * gs * 0.2f + nx * gs * 0.12f * g.swirl;
            p.vx += ax * a;
            if (p.kind != KRain) p.vy += ay * a;
            if (p.kind == KLeaf)
            {
                p.rotv = std::max(-500.0f, std::min(500.0f, p.rotv + g.swirl * gs / S * f * dt * 0.7f));
                p.rest = std::max(p.rest, 0.6f);
                if (f > 0.5f && p.mode != Tumble && Rand() < dt * 1.5f)
                {
                    // caught properly by the gust: it tumbles for a moment
                    p.mode = Tumble;
                    p.freq2 = Range(4.0f, 9.0f) * g.swirl;
                    p.modeTime = Range(0.8f, 2.0f);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- snow settling

// Lying snow never just fills the space up: the deeper it gets, the faster it packs down and sublimates,
// at a rate that wanders slowly along the surface so drifts grow in some places and shrink in others.
// Now and then a stretch of the taskbar drift is blown away over a few seconds, lifting a few flakes.
void Sim::SettleSnow(SnowCap& c, float left, bool isGround, float dt)
{
    const float B = float(SnowCap::Bucket);
    int n = int(c.height.size());
    if (n == 0 || c.max <= 0) return;

    if (isGround)
    {
        // start a new blow-away now and then, somewhere there is a fair bit of snow
        if (snowGusts.size() < 3 && Rand() < dt * 0.25f)
        {
            int j = int(Rand() * n) % n;
            if (c.height[j] > c.max * 0.25f)
            {
                SnowGust g;
                g.x = (j + 0.5f) * B;
                g.w = Range(120.0f, 320.0f) * ScaleAt(g.x);
                g.strength = Range(1.5f, 3.0f);
                g.life = Range(3.0f, 6.0f);
                g.age = 0;
                snowGusts.push_back(g);
            }
        }
        for (size_t i = snowGusts.size(); i-- > 0;)
        {
            snowGusts[i].age += dt;
            if (snowGusts[i].age >= snowGusts[i].life) { snowGusts[i] = snowGusts.back(); snowGusts.pop_back(); }
        }
    }

    for (int j = 0; j < n; j++)
    {
        float h = c.height[j];
        if (h <= 0) continue;
        float x = left + (j + 0.5f) * B;
        float s = ScaleAt(x);
        float r = h / c.max;
        // slowly drifting unevenness, 0.3..1.5
        float wander = 0.9f + 0.6f * std::sin(x * 0.0040f / s + time * 0.07f) * std::sin(x * 0.0013f / s - time * 0.05f + 1.7f);
        float loss = 2.0f * s * wander * r * r;
        if (isGround)
        {
            for (const SnowGust& g : snowGusts)
            {
                float d = std::fabs(x - g.x) / (g.w * 0.5f);
                if (d >= 1) continue;
                float env = std::sin(Pi * g.age / g.life);   // builds up and dies down
                float shape = 0.5f + 0.5f * std::cos(Pi * d);
                loss += g.strength * s * shape * env * (0.3f + r);
                // a few flakes lifted off the top
                if (Rand() < dt * 2 * shape * env && parts.size() + pending.size() < MaxParticles)
                {
                    Particle p = NewSnow(0);
                    p.x = x;
                    p.y = GroundAt(x) - c.mud[j] - h - 1;
                    p.vx = Range(-40.0f, 40.0f) * s + mix.windPx * s * 0.5f;
                    p.vy = -Range(30.0f, 110.0f) * s;
                    pending.push_back(p);
                }
            }
        }
        c.height[j] = std::max(0.0f, h - loss * dt);
    }
}
// Snow on a window's top edge creeps slowly towards the corners, faster the deeper it lies. At each corner
// it builds a small overhang; once that gets heavy enough it breaks off and drops as a clump of flakes that
// spreads out as it falls.
void Sim::SlideSnowOffEdges(SnowCap& c, const Win& w, float dt)
{
    int n = int(c.height.size());
    if (n < 4) return;
    const int reach = std::min(n / 2, 12);   // buckets from each end that slide outwards
    for (int side = 0; side < 2; side++)
    {
        float& cornice = side ? c.corniceR : c.corniceL;
        for (int k = reach - 1; k >= 0; k--)
        {
            int j = side ? n - 1 - k : k;
            int out = side ? j + 1 : j - 1;
            float h = c.height[j];
            if (h <= 0) continue;
            float m = h * std::min(1.0f, dt * 0.35f * (0.2f + h / c.max));
            c.height[j] -= m;
            if (out < 0 || out >= n) cornice += m;   // over the edge: onto the overhang
            else c.height[out] += m;
        }

        float s = ScaleAt(side ? w.r : w.l);
        if (cornice > 6 * s && Rand() < dt * (cornice / (6 * s) - 0.8f))
        {
            // the overhang breaks off: a clump that falls together and slowly comes apart
            int count = std::min(30, 3 + int(cornice / (1.2f * s)));
            float dir = side ? 1.0f : -1.0f;
            float cx = (side ? w.r : w.l) + dir * 3 * s, cy = w.t - 2 * s;
            float vx = dir * Range(15.0f, 50.0f) * s, vy = Range(10.0f, 50.0f) * s;
            for (int i = 0; i < count && parts.size() + pending.size() < MaxParticles; i++)
            {
                Particle p = NewSnow(0);
                p.x = cx + Range(-4.0f, 4.0f) * s;
                p.y = cy + Range(-3.0f, 3.0f) * s;
                p.vx = vx + Range(-12.0f, 12.0f) * s;
                p.vy = vy + Range(-10.0f, 10.0f) * s;
                p.size *= Range(1.0f, 1.5f);
                pending.push_back(p);
            }
            cornice = 0;
        }
        cornice = std::min(cornice, 14 * s);
    }
}