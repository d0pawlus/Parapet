#include "app.h"
#include <algorithm>

// Rare visitors: small hand-drawn creatures and plants that turn up now and then on the taskbar or on
// window tops, only when the weather suits them, and that react to the mouse pointer.

namespace
{
    const float Pi = 3.14159265f;

    // chance per second of each visitor appearing while its conditions hold (a few minutes apart on average)
    const float Chance[CritterKinds] = {
        1 / 240.0f,   // hedgehog
        1 / 200.0f,   // caterpillar
        1 / 150.0f,   // snow fox
        1 / 90.0f,    // snowdrops
        1 / 200.0f,   // snail
        1 / 180.0f,   // frog
        1 / 300.0f,   // robin
        1 / 300.0f,   // squirrel
        1 / 150.0f,   // mushrooms
    };
}

// ---------------------------------------------------------------- queries

float Sim::SurfaceY(float x) const
{
    int n = int(groundY.size());
    int i = std::max(0, std::min(n - 1, int(x / SnowCap::Bucket)));
    return groundY[i] - ground.mud[i] - ground.height[i];
}

bool Sim::CursorNear(float x, float y, float radius) const
{
    if (!cursor.valid) return false;
    float dx = cursor.x1 - x, dy = cursor.y1 - y;
    return dx * dx + dy * dy < radius * radius;
}

int Sim::NearestGroundLeaf(float x, float range) const
{
    int best = -1;
    float bestD = range;
    for (size_t i = 0; i < parts.size(); i++)
    {
        const Particle& p = parts[i];
        if (p.kind != KLeaf || p.state != Resting || !p.onGround || p.hidden) continue;
        float d = std::fabs(p.x - x);
        if (d < bestD) { bestD = d; best = int(i); }
    }
    return best;
}

int Sim::RestingGroundLeaves() const
{
    int n = 0;
    for (const Particle& p : parts)
        if (p.kind == KLeaf && p.state == Resting && p.onGround && !p.hidden) n++;
    return n;
}

bool Sim::PuddleAt(float x, float& depth) const
{
    int i = std::max(0, std::min(int(ground.water.size()) - 1, int(x / SnowCap::Bucket)));
    depth = ground.water[i];
    return depth > 1.5f * S;
}

float Sim::FindPuddle(float from, float dir) const
{
    int n = int(ground.water.size());
    int start = int(from / SnowCap::Bucket);
    int reach = int(500.0f * S / SnowCap::Bucket);
    for (int k = int(40.0f * S / SnowCap::Bucket); k < reach; k++)
    {
        int i = start + int(dir) * k;
        if (i < 0 || i >= n) break;
        if (ground.water[i] > 2.0f * S) return (i + 0.5f) * SnowCap::Bucket;
    }
    return -1;
}

// ---------------------------------------------------------------- spawning

void Sim::SpawnCritter(int kind, bool forced)
{
    if (kind < 0) kind = int(Rand() * CritterKinds) % CritterKinds;
    if ((!forced && critters.size() >= 6) || groundY.empty()) return;

    Critter c = {};
    c.kind = uint8_t(kind);
    c.alpha = 1;
    c.seed = Range(0, 100);
    c.dir = Rand() < 0.5f ? -1.0f : 1.0f;
    c.face = c.dir;
    c.blink = Range(1.0f, 4.0f);
    float edgeX = c.dir > 0 ? -60.0f * S : W + 60.0f * S;
    c.x = edgeX;

    switch (kind)
    {
    case Caterpillar:
    {
        // start poked the leaves it is going to eat
        int leaf = NearestGroundLeaf(W * 0.5f, float(W));
        if (leaf >= 0) c.x = std::max(10.0f * S, std::min(W - 10.0f * S, parts[leaf].x - c.dir * Range(120, 260) * S));
        else c.x = Range(0.1f, 0.9f) * W;
        c.alpha = 0;
        break;
    }
    case SnowFox:
    {
        size_t best = 0;
        for (size_t i = 0; i < ground.height.size(); i++)
            if (ground.height[i] > ground.height[best]) best = i;
        if (ground.height[best] < 12.0f * S)
        {
            if (!forced) return;
            // summoned without a drift: heap one up
            best = size_t(Range(0.2f, 0.8f) * ground.height.size());
            float cx = (best + 0.5f) * SnowCap::Bucket;
            for (int k = -30; k <= 30; k++) ground.Add(cx + k * 2.0f * S, 1.6f * S * (1 - std::fabs(k / 31.0f)));
            for (int k = 0; k < 10; k++) ground.Add(cx + Range(-50, 50) * S, 2.0f * S);
        }
        c.x = (best + 0.5f) * SnowCap::Bucket;
        c.grow = 0;
        break;
    }
    case Snowdrops:
    case Mushrooms:
    {
        c.x = Range(0.1f, 0.9f) * W;
        if (kind == Mushrooms)
        {
            // where the ground is muddiest
            size_t best = size_t(c.x / SnowCap::Bucket);
            for (size_t i = 0; i < ground.mud.size(); i++)
                if (ground.mud[i] > ground.mud[best] + 0.5f * S) best = i;
            c.x = (best + 0.5f) * SnowCap::Bucket;
        }
        c.count = kind == Snowdrops ? 3 + int(Rand() * 3) : 2 + int(Rand() * 2);
        c.timer = kind == Snowdrops ? 150.0f : 180.0f;
        c.angle = 0;
        c.vx = Rand();   // mushrooms: which kind (fly agaric or bolete)
        break;
    }
    case Snail:
    {
        float px = -1;
        for (int k = 0; k < 8 && px < 0; k++) px = FindPuddle(Range(0.1f, 0.9f) * W, c.dir);
        c.x = px > 0 ? px - c.dir * 60.0f * S : Range(0.1f, 0.9f) * W;
        c.alpha = 0;
        c.grow = 1;
        break;
    }
    case Frog:
    {
        float px = -1;
        for (int k = 0; k < 8 && px < 0; k++) px = FindPuddle(Range(0.1f, 0.9f) * W, c.dir);
        c.x = px > 0 ? px + c.dir * 30.0f * S : Range(0.15f, 0.85f) * W;
        c.timer = Range(2, 4);
        c.alpha = 0;
        break;
    }
    case Robin:
    {
        // fly in to a visible window top, or to the taskbar if there is none
        c.host = nullptr;
        if (tracker)
        {
            std::vector<const Win*> roofs;
            for (const Win& w : tracker->all)
                if (w.surface && w.r - w.l > 200 * S && w.t > 60 * S && !tracker->Covered((w.l + w.r) / 2, w.t - 4, w.z)) roofs.push_back(&w);
            if (!roofs.empty())
            {
                const Win* w = roofs[size_t(Rand() * roofs.size()) % roofs.size()];
                c.host = w->h;
                c.offx = Range(40 * S, w->r - w->l - 40 * S);
            }
        }
        if (!c.host) c.offx = Range(0.2f, 0.8f) * W;
        c.y = Range(0.1f, 0.4f) * H;
        break;
    }
    default:
        break;
    }
    if (c.kind != Robin) c.y = SurfaceY(c.x);
    critters.push_back(c);
}

void Sim::UpdateCritters(float dt, const Tracker& tr)
{
    critterClock += dt;
    if (critterClock >= 1.0f)
    {
        critterClock -= 1.0f;
        if (rareEvents && critters.size() < 2)
        {
            float maxSnow = 0, maxWater = 0, maxMud = 0;
            for (size_t i = 0; i < ground.height.size(); i++)
            {
                maxSnow = std::max(maxSnow, ground.height[i]);
                maxWater = std::max(maxWater, ground.water[i]);
                maxMud = std::max(maxMud, ground.mud[i]);
            }
            SYSTEMTIME now;
            GetLocalTime(&now);
            bool autumn = now.wMonth >= 9 && now.wMonth <= 11, lateWinter = now.wMonth == 2 || now.wMonth == 3;
            bool snowdropSeason = now.wMonth >= 1 && now.wMonth <= 4;
            bool snowing = mix.snow > 0;
            float t = mix.tempC;
            int leaves = RestingGroundLeaves();
            bool ok[CritterKinds] = {
                !snowing && maxSnow < 3 * S && t > 3 && (mix.leaves > 0 || leaves > 3),   // hedgehog
                leaves >= 3 && !snowing && t > 2,                                          // caterpillar
                maxSnow > 18 * S,                                                          // snow fox
                // only in their real season: a thaw from January to April, or any mild day in February-March
                snowdropSeason && !snowing && t > 2 && (maxSnow > 2 * S || lateWinter),    // snowdrops
                maxWater > 1 * S && mix.rain < 0.6f && t > 4,                              // snail
                maxWater > 4 * S && t > 5,                                                 // frog
                mix.rain < 0.6f,                                                           // robin
                !snowing && (autumn || mix.leaves > 0),                                    // squirrel
                maxMud > 2.5f * S && t > 3,                                                // mushrooms
            };
            for (int k = 0; k < CritterKinds; k++)
            {
                bool present = false;
                for (const Critter& c : critters) present |= c.kind == k;
                if (ok[k] && !present && Rand() < Chance[k]) { SpawnCritter(k, false); break; }
            }
        }
    }
    for (size_t i = critters.size(); i-- > 0;)
    {
        if (!UpdateCritter(critters[i], dt, tr)) critters.erase(critters.begin() + i);
    }
}

// ---------------------------------------------------------------- behaviour

bool Sim::UpdateCritter(Critter& c, float dt, const Tracker& tr)
{
    const float S = ScaleAt(c.x);   // sizes and speeds follow the monitor the visitor is on
    c.t += dt;
    bool offscreen = c.x < -100 * S || c.x > W + 100 * S;
    auto onGround = [&]() {
        c.y = SurfaceY(c.x);
        c.hidden = tr.Covered(c.x, c.y - 6 * S, INT32_MAX, true);
    };
    auto fadeIn = [&]() { c.alpha = std::min(1.0f, c.alpha + dt); };
    float cursorSpeed = cursor.valid ? std::sqrt(cursor.vx * cursor.vx + cursor.vy * cursor.vy) : 0;

    // shared liveliness: smooth turning, blinking, glancing at the pointer, settling after a squash
    c.face += (c.dir - c.face) * std::min(1.0f, dt * 7);
    c.blink -= dt;
    if (c.blink < -0.14f) c.blink = Range(1.5f, 5.0f);
    c.squash = std::max(0.0f, c.squash - dt * 4);
    float lookTarget = 0;
    if (cursor.valid)
    {
        float dx = cursor.x1 - c.x, dy = cursor.y1 - c.y;
        if (dx * dx + dy * dy < 420 * S * 420 * S) lookTarget = std::max(-1.0f, std::min(1.0f, dx / (250 * S)));
    }
    c.look += (lookTarget - c.look) * std::min(1.0f, dt * 4);

    switch (c.kind)
    {
    case Hedgehog:
    {
        // waddles along, snuffles, ploughs through leaf heaps picking leaves up on its spines;
        // poked, it rolls into a trembling ball, then peeks out before carrying on
        bool poked = CursorNear(c.x, c.y - 10 * S, 150 * S);
        if (poked && c.state != 2) { c.state = 2; c.timer = Range(3, 5); c.squash = 1; }
        switch (c.state)
        {
        case 0:
        {
            c.anim += dt * 9;
            c.x += c.dir * 30 * S * dt;
            if (Rand() < dt * 0.18f) { c.state = 1; c.timer = Range(1.0f, 2.5f); }
            // curious about a slow pointer nearby
            if (cursor.valid && cursorSpeed < 100 * S && CursorNear(c.x, c.y, 350 * S) && Rand() < dt * 0.5f) { c.state = 1; c.timer = 1.5f; }
            for (size_t i = 0; i < parts.size(); i++)
            {
                Particle& p = parts[i];
                if (p.kind != KLeaf || p.state != Resting || !p.onGround || p.heaped) continue;
                float ahead = (p.x - c.x) * c.dir;
                // a nudge with the snout: the leaf slides a little way ahead and comes to rest again
                if (ahead > 4 * S && ahead < 14 * S && std::fabs(p.vx) < 10 * S)
                {
                    p.vx = c.dir * Range(70, 130) * S;
                    p.rest = std::max(p.rest, 20.0f);
                }
            }
            int leaf = c.count < 4 ? NearestGroundLeaf(c.x + c.dir * 8 * S, 10 * S) : -1;
            if (leaf >= 0 && Rand() < dt * 1.5f)
            {
                c.carried[c.count++] = parts[leaf].color;
                parts[leaf].state = Fading;
                parts[leaf].alpha = 0.001f;
            }
            break;
        }
        case 1:
            c.aux += dt * 25;   // nose wiggle
            c.timer -= dt;
            if (c.timer <= 0) c.state = 0;
            break;
        case 2:
            c.timer -= dt;
            c.aux2 = poked ? 1.0f : std::max(0.0f, c.aux2 - dt * 2);   // trembling while the pointer hovers
            if (poked) c.timer = std::max(c.timer, 1.0f);
            if (c.timer <= 0) { c.state = 3; c.timer = 0.9f; }
            break;
        default:
            c.timer -= dt;
            if (c.timer <= 0) c.state = 0;
            break;
        }
        float curl = c.state == 2 ? 1.0f : c.state == 3 ? 0.55f : 0.0f;
        c.grow += (curl - c.grow) * std::min(1.0f, dt * 7);
        onGround();
        return !(c.t > 2 && offscreen);
    }
    case Caterpillar:
    {
        // inches towards the nearest fallen leaf, munches it away crumb by crumb, wanders off after a few
        fadeIn();
        bool poked = CursorNear(c.x, c.y - 6 * S, 120 * S);
        if (poked && c.state != 2 && c.state != 3) { c.state = 2; c.timer = 2.5f; }
        if (c.t > 240 && c.state != 3) c.state = 3;
        if (c.state == 0 || c.state == 3)
        {
            if (c.state == 0)
            {
                int leaf = NearestGroundLeaf(c.x, float(W));
                if (leaf < 0) c.state = 3;
                else
                {
                    float d = parts[leaf].x - c.x;
                    c.dir = d > 0 ? 1.0f : -1.0f;
                    if (std::fabs(d) < 7 * S) { c.state = 1; c.timer = 5; }
                }
            }
            if (c.state == 3 && c.t < 3) c.dir = c.x < W / 2 ? -1.0f : 1.0f;
            c.anim += dt * 2.4f;
            c.x += c.dir * 26 * S * std::max(0.0f, std::sin(c.anim)) * dt;   // moves only while stretching
        }
        else if (c.state == 1)
        {
            c.anim += dt * 10;
            c.timer -= dt;
            int leaf = NearestGroundLeaf(c.x, 16 * S);
            if (leaf >= 0)
            {
                Particle& p = parts[leaf];
                p.size *= 1 - dt * 0.12f;
                p.rest = std::max(p.rest, 3.0f);
                if (Rand() < dt * 5) Splash(c.x + c.dir * 7 * S, c.y - 3 * S, 0.5f, 2, 1, 0.35f);   // crumbs
            }
            if (c.timer <= 0 || leaf < 0)
            {
                if (leaf >= 0) { parts[leaf].state = Fading; parts[leaf].alpha = 0.001f; }
                c.count++;
                c.state = c.count >= 3 ? 3 : 0;
            }
        }
        else
        {
            c.aux += dt * 14;   // wobble while curled up
            c.timer -= dt;
            if (c.timer <= 0) c.state = 0;
        }
        onGround();
        return !(c.state == 3 && offscreen);
    }
    case SnowFox:
    {
        // pops out of a drift in a puff of snow, listens with its head cocked, wiggles, pounces in an arc
        // and dives head first back into the snow
        auto puff = [&](int n, float up) {
            for (int k = 0; k < n && parts.size() < 7000; k++)
            {
                Particle p = NewSnow(0);
                p.x = c.x + Range(-14, 14) * S;
                p.y = c.y - 2;
                p.vx = c.vx * 0.2f + Range(-120, 120) * S;
                p.vy = -Range(up * 0.4f, up) * S;
                parts.push_back(p);
            }
        };
        if (c.state == 0)
        {
            if (c.grow == 0) { onGround(); puff(14, 260); }
            c.grow = std::min(1.0f, c.grow + dt / 0.7f);
            if (c.grow >= 1) { c.state = 1; c.timer = Range(1.5f, 3.0f); }
            onGround();
        }
        else if (c.state == 1)
        {
            c.anim += dt;
            c.aux = std::sin(c.t * 1.7f) * 12;                         // head cocked, listening
            c.aux2 = std::sin(c.t * 9) > 0.8f ? 1.0f : 0.0f;           // ear twitch
            c.timer -= dt;
            if (CursorNear(c.x, c.y - 20 * S, 220 * S)) { c.timer = 0; c.dir = cursor.x1 > c.x ? -1.0f : 1.0f; }
            if (c.timer <= 0) { c.state = 4; c.timer = CursorNear(c.x, c.y - 20 * S, 220 * S) ? 0.2f : 0.6f; }
            onGround();
        }
        else if (c.state == 4)
        {
            // crouch and wiggle before the pounce
            c.squash = 1;
            c.aux = 0;
            c.timer -= dt;
            if (c.timer <= 0)
            {
                c.state = 2;
                c.vx = c.dir * Range(180, 280) * S;
                c.vy = -Range(480, 600) * S;
            }
            onGround();
        }
        else if (c.state == 2)
        {
            c.vy += 1400 * S * dt;
            c.x += c.vx * dt;
            c.y += c.vy * dt;
            c.angle = std::atan2(c.vy, std::fabs(c.vx)) * 180 / Pi;
            float gy = SurfaceY(c.x);
            if (c.vy > 0 && c.y >= gy)
            {
                c.y = gy;
                c.state = 3;
                puff(30, 380);
            }
            c.hidden = tr.Covered(c.x, c.y - 10 * S, INT32_MAX, true);
        }
        else
        {
            c.grow -= dt / 0.45f;
            c.angle = 70;
            c.aux2 = std::sin(c.t * 30) * 8;   // a last flick of the tail
            if (c.grow <= 0)
            {
                if (c.count == 0 && Rand() < 0.5f)
                {
                    // and once more, from a little further along
                    c.count = 1;
                    c.x = std::max(30 * S, std::min(W - 30 * S, c.x + c.dir * Range(120, 300) * S));
                    c.state = 0;
                    c.grow = 0;
                    c.angle = 0;
                    c.vx = 0;
                }
                else return false;
            }
        }
        return !offscreen;
    }
    case Snowdrops:
    case Mushrooms:
    {
        // grow up, sway in the wind, spring back when brushed by the pointer, then wilt
        c.timer -= dt;
        float wilt = c.kind == Snowdrops ? 20.0f : 15.0f, rise = c.kind == Snowdrops ? 15.0f : 10.0f;
        if (c.timer > wilt) c.grow = std::min(1.0f, c.grow + dt / rise);
        else c.alpha = std::max(0.0f, c.timer / wilt);
        float target = std::sin(c.t * 1.3f + c.seed) * 4 + mix.windPx * 0.02f;
        if (CursorNear(c.x, c.y - 20 * S, 130 * S))
        {
            target += std::max(-30.0f, std::min(30.0f, cursor.vx / S * 0.03f));
            if (c.kind == Mushrooms && cursorSpeed > 200 * S) c.squash = 1;   // boing
        }
        c.vy += ((target - c.angle) * 30 - c.vy * 4) * dt;   // a springy stem
        c.angle += c.vy * dt;
        onGround();
        return c.timer > 0;
    }
    case Snail:
    {
        // crawls very slowly after the rain leaving a glistening trail; hides in its shell when disturbed
        if (c.t > 300) c.alpha -= dt / 3;
        else fadeIn();
        bool poked = CursorNear(c.x, c.y - 6 * S, 130 * S);
        if (poked) { c.state = 1; c.timer = 4; }
        if (c.state == 1) { c.timer -= dt; if (c.timer <= 0) c.state = 0; }
        c.grow += ((c.state == 0 ? 1.0f : 0.0f) - c.grow) * std::min(1.0f, dt * (c.state == 1 ? 9.0f : 1.5f));
        if (c.state == 0 && c.grow > 0.9f)
        {
            float step = c.dir * 7 * S * (0.7f + 0.3f * std::sin(c.t * 2)) * dt;
            c.x += step;
            c.anim += std::fabs(step);
            c.aux += dt * 3;   // the foot ripples as it glides
            if (c.anim > 5 * S)
            {
                c.anim = 0;
                c.trail.push_back(D2D1::Point2F(c.x - c.dir * 6 * S, c.y - 0.6f * S));
                c.trailAge.push_back(0);
            }
        }
        for (float& a : c.trailAge) a += dt;
        while (!c.trailAge.empty() && (c.trailAge.front() > 40 || c.trail.size() > 300))
        {
            c.trail.erase(c.trail.begin());
            c.trailAge.erase(c.trailAge.begin());
        }
        onGround();
        return c.alpha > 0 && !offscreen;
    }
    case Frog:
    {
        // sits by the puddles with a pulsing throat, snaps passing drops and flakes with its tongue,
        // hops from puddle to puddle (crouch, leap, plop) and leaps away when poked
        fadeIn();
        bool leaving = c.count > 6 || c.t > 90;
        c.aux = std::max(0.0f, c.aux - dt);     // tongue out
        c.aux2 = std::max(0.0f, c.aux2 - dt);   // tongue cooldown
        if (c.state == 0)
        {
            c.anim += dt * 6;
            c.timer -= dt;
            bool poked = CursorNear(c.x, c.y - 8 * S, 160 * S);
            if (poked) { c.dir = cursor.x1 > c.x ? -1.0f : 1.0f; c.timer = 0; }
            if (c.aux2 <= 0 && !poked)
            {
                for (Particle& p : parts)
                {
                    if (p.state != Falling || (p.kind != KSnow && p.kind != KRain && p.kind != KLeaf)) continue;
                    float dx = (p.x - c.x) * c.dir, dy = p.y - c.y;
                    if (dx > 6 * S && dx < 60 * S && dy > -70 * S && dy < -6 * S)
                    {
                        c.vx = dx / S;
                        c.vy = dy / S;
                        c.aux = 0.25f;
                        c.aux2 = Range(1.5f, 4.0f);
                        p.y = float(H) + 5000;   // gulp
                        break;
                    }
                }
            }
            if (c.timer <= 0) { c.state = 3; c.timer = poked ? 0.08f : 0.2f; }
            onGround();
        }
        else if (c.state == 3)
        {
            c.squash = 1;   // crouch
            c.timer -= dt;
            if (c.timer <= 0)
            {
                bool poked = CursorNear(c.x, c.y - 8 * S, 160 * S);
                float target = leaving ? -1 : FindPuddle(c.x, c.dir);
                if (target < 0 && !leaving && Rand() < 0.5f) { c.dir = -c.dir; target = FindPuddle(c.x, c.dir); }
                float dx = target > 0 ? target - c.x : c.dir * Range(60, 160) * S * (poked || leaving ? 2.0f : 1.0f);
                if (target > 0) c.dir = dx > 0 ? 1.0f : -1.0f;
                float T = 0.55f;
                c.vx = dx / T;
                c.vy = -0.5f * 1600 * S * T;
                c.state = 1;
                c.count++;
            }
            onGround();
        }
        else
        {
            c.vy += 1600 * S * dt;
            c.x += c.vx * dt;
            c.y += c.vy * dt;
            c.angle = std::atan2(c.vy, std::fabs(c.vx) + 1) * 180 / Pi * 0.5f;
            float gy = SurfaceY(c.x);
            if (c.vy > 0 && c.y >= gy)
            {
                c.y = gy;
                c.state = 0;
                c.angle = 0;
                c.squash = 1;
                c.vx = c.vy = 0;
                c.timer = leaving ? 0.1f : Range(1.5f, 5.0f);
                float depth;
                if (PuddleAt(c.x, depth))
                {
                    // plop!
                    Splash(c.x, gy - depth, 1.0f, 0, 6, 1.4f);
                    Ripple(c.x, gy - depth, 1.4f);
                    int b = std::max(0, std::min(int(ground.waveV.size()) - 1, int(c.x / SnowCap::Bucket)));
                    ground.waveV[b] -= 60 * S;
                }
            }
            c.hidden = tr.Covered(c.x, c.y - 8 * S, INT32_MAX, true);
        }
        return !offscreen;
    }
    case Robin:
    {
        // flies in, lands with a little bob, hops and pecks along a window's top edge, flicks its tail,
        // cocks its head at the pointer, and flies off when it comes too close or the window moves
        const Win* w = c.host ? tr.Get(c.host) : nullptr;
        if (c.host && (!w || !w->surface) && c.state != 2) { c.state = 2; c.vx = c.dir * 320 * S; c.vy = -200 * S; }
        auto perch = [&](float& tx, float& ty) {
            if (w) { tx = w->l + c.offx; ty = w->t - CapAt(*w, tx); }
            else { tx = c.offx; ty = SurfaceY(tx); }
        };
        if (c.state == 0)
        {
            float tx, ty;
            perch(tx, ty);
            float dx = tx - c.x, dy = ty - c.y, d = std::sqrt(dx * dx + dy * dy);
            c.anim += dt * 18;
            c.dir = dx > 0 ? 1.0f : -1.0f;
            float v = std::min(d / dt, 420 * S);
            if (d < 6 * S) { c.state = 1; c.timer = Range(15, 35); c.x = tx; c.y = ty; c.squash = 1; }
            else { c.x += dx / d * v * dt; c.y += dy / d * v * dt + std::sin(c.t * 9) * 20 * S * dt; }
            c.hidden = false;
        }
        else if (c.state == 1)
        {
            c.timer -= dt;
            c.grow = std::max(0.0f, c.grow - dt * 4);     // hop arc
            c.anim = std::max(0.0f, c.anim - dt * 5);     // peck
            c.aux2 = std::max(0.0f, c.aux2 - dt * 4);     // tail flick
            if (c.grow <= 0 && c.anim <= 0)
            {
                float r = Rand();
                if (r < dt * 0.7f)
                {
                    float span = w ? w->r - w->l : float(W);
                    c.offx = std::max(20 * S, std::min(span - 20 * S, c.offx + Range(-20, 20) * S));
                    if (Rand() < 0.3f) c.dir = -c.dir;
                    c.grow = 1;
                }
                else if (r < dt * 1.6f) c.anim = 1;
                else if (r < dt * 2.2f) c.aux2 = 1;
            }
            float tx, ty;
            perch(tx, ty);
            c.x = tx;
            c.y = ty;
            c.hidden = w ? tr.Covered(c.x, c.y - 4 * S, w->z) : tr.Covered(c.x, c.y - 4 * S, INT32_MAX, true);
            bool scared = CursorNear(c.x, c.y - 8 * S, 200 * S) || (w && Fast(*w, dt));
            if (scared || c.timer <= 0 || c.hidden)
            {
                c.state = 2;
                c.dir = cursor.valid && cursor.x1 > c.x ? -1.0f : (scared ? c.dir : (Rand() < 0.5f ? -1.0f : 1.0f));
                c.face = c.dir;
                c.vx = c.dir * Range(250, 400) * S;
                c.vy = -Range(150, 300) * S;
                c.hidden = false;
            }
        }
        else
        {
            c.anim += dt * 22;
            c.vy -= 200 * S * dt;
            c.x += c.vx * dt;
            c.y += c.vy * dt;
            if (c.y < -60 * S) return false;
        }
        return !(c.state == 2 && offscreen);
    }
    case Squirrel:
    {
        // bounds along with an acorn, sits up to nibble it with its tail twitching; a nearby pointer makes it
        // freeze, stare, and then dash off the other way
        bool poked = CursorNear(c.x, c.y - 10 * S, 170 * S);
        c.aux2 = std::max(0.0f, c.aux2 - dt * 3);
        if (poked && c.state == 0 && c.timer > -1) { c.state = 2; c.timer = 0.35f; }
        if (c.state == 0)
        {
            bool dashing = c.timer < -0.5f;
            c.anim += dt * (dashing ? 22 : 14);
            c.x += c.dir * (dashing ? 520 : 240) * S * dt;
            c.timer = std::min(c.timer + dt, 0.0f);
            if (!poked && !dashing && Rand() < dt * 0.25f) { c.state = 1; c.timer = Range(1.5f, 3.0f); }
        }
        else if (c.state == 1)
        {
            c.aux += dt * 12;   // nibbling
            if (Rand() < dt * 1.2f) c.aux2 = 1;
            c.timer -= dt;
            if (poked) { c.state = 2; c.timer = 0.35f; }
            else if (c.timer <= 0) c.state = 0;
        }
        else
        {
            // frozen, staring at the pointer
            c.timer -= dt;
            if (c.timer <= 0)
            {
                c.state = 0;
                if (cursor.valid) c.dir = cursor.x1 > c.x ? -1.0f : 1.0f;
                c.timer = -2.5f;   // dash for a while
            }
        }
        onGround();
        return !(c.t > 2 && offscreen);
    }
    }
    return false;
}

// ---------------------------------------------------------------- drawing

void Renderer::Fill(float cx, float cy, float rx, float ry, D2D1_COLOR_F c)
{
    paint->SetColor(c);
    dc->FillEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), rx, ry), paint.Get());
}

void Renderer::Stroke(float x0, float y0, float x1, float y1, float w, D2D1_COLOR_F c)
{
    paint->SetColor(c);
    dc->DrawLine(D2D1::Point2F(x0, y0), D2D1::Point2F(x1, y1), paint.Get(), w);
}

void Renderer::Poly(const D2D1_POINT_2F* pts, int n, D2D1_COLOR_F c)
{
    ComPtr<ID2D1PathGeometry> g;
    ComPtr<ID2D1GeometrySink> s;
    factory->CreatePathGeometry(&g);
    g->Open(&s);
    s->BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
    s->AddLines(pts + 1, UINT32(n - 1));
    s->EndFigure(D2D1_FIGURE_END_CLOSED);
    s->Close();
    paint->SetColor(c);
    dc->FillGeometry(g.Get(), paint.Get());
}

// Each visitor is drawn in its own local space: 1 unit = 1 px at 1080p, origin at its feet, facing +x, up is -y.
// Flat warm colours, a soft shadow underneath, eyes with a highlight that blink.
void Renderer::DrawCritters(const Sim& sim, const Tracker& tr)
{
    if (sim.critters.empty()) return;
    if (!paint) dc->CreateSolidColorBrush(D2D1::ColorF(0, 0, 0, 1), &paint);
    const float CS = 1.6f;   // visitors are drawn a bit larger than life so they read on the screen
    static const D2D1_COLOR_F LeafFlat[Sim::LeafColors] = {
        { 0.77f, 0.28f, 0.12f, 1 }, { 0.87f, 0.47f, 0.12f, 1 }, { 0.92f, 0.67f, 0.16f, 1 }, { 0.63f, 0.16f, 0.12f, 1 },
        { 0.80f, 0.59f, 0.24f, 1 }, { 0.55f, 0.35f, 0.17f, 1 }, { 0.90f, 0.75f, 0.27f, 1 }, { 0.71f, 0.39f, 0.16f, 1 },
    };
    dc->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

    for (const Critter& c : sim.critters)
    {
        if (c.hidden || c.alpha <= 0.01f) continue;
        const float S = sim.ScaleAt(c.x), sc = S * k * CS;
        float a = c.alpha;
        auto C = [&](float r, float g, float b, float al = 1) { return D2D1::ColorF(r, g, b, al * a); };
        // facing eases through zero when turning round, which reads as the creature turning
        float face = std::fabs(c.face) < 0.15f ? (c.face < 0 ? -0.15f : 0.15f) : c.face;
        float lookFwd = c.look * (c.face < 0 ? -1.0f : 1.0f);   // pointer in front (+) or behind (-)
        auto place = [&](float dy, float angle, float sy = 1.0f, float dx = 0.0f) {
            return D2D1::Matrix3x2F::Scale(face * sc, sc * sy) * D2D1::Matrix3x2F::Rotation(angle * (face < 0 ? -1.0f : 1.0f)) *
                   D2D1::Matrix3x2F::Translation((c.x + dx * S * CS) * k, (c.y + dy * S * CS) * k);
        };
        D2D1_MATRIX_3X2_F T = place(0, c.angle);
        dc->SetTransform(T);
        auto rotated = [&](float deg, float px, float py) { dc->SetTransform(D2D1::Matrix3x2F::Rotation(deg, D2D1::Point2F(px, py)) * T); };
        auto unrotate = [&]() { dc->SetTransform(T); };
        auto eye = [&](float x, float y, float r, D2D1_COLOR_F iris) {
            if (c.blink < 0) { Stroke(x - r, y + r * 0.2f, x + r, y + r * 0.2f, r * 0.55f, iris); return; }
            Fill(x, y, r, r, iris);
            Fill(x - r * 0.35f + lookFwd * r * 0.15f, y - r * 0.35f, r * 0.38f, r * 0.38f, C(1, 1, 1, 0.9f));
        };
        auto shadow = [&](float rx) { Fill(0, 0.6f, rx, 2.2f, C(0.1f, 0.07f, 0.05f, 0.16f)); };
        float breathe = 1 + 0.025f * std::sin(c.t * 3.1f + c.seed);

        switch (c.kind)
        {
        case Hedgehog:
        {
            D2D1_COLOR_F spine = C(0.42f, 0.30f, 0.20f), dark = C(0.26f, 0.18f, 0.12f), tip = C(0.74f, 0.60f, 0.42f);
            D2D1_COLOR_F face = C(0.88f, 0.76f, 0.60f), nose = C(0.14f, 0.09f, 0.07f), blush = C(0.93f, 0.58f, 0.52f, 0.5f);
            float curl = c.grow;
            shadow(curl > 0.5f ? 13.0f : 17.0f);
            if (curl > 0.5f)
            {
                // a spiky ball; trembles while the pointer hovers, peeks out with one eye when it is gone
                float shake = std::sin(c.t * 60) * c.aux2 * 0.8f;
                T = place(-c.squash * 1.5f, 0, 1, shake);
                dc->SetTransform(T);
                Fill(0, -11.5f, 12.5f, 11.5f, spine);
                for (int i = 0; i < 28; i++)
                {
                    float ang = i * 2 * Pi / 28 + c.seed;
                    float cs = std::cos(ang), sn = std::sin(ang);
                    Stroke(cs * 9.5f, -11.5f + sn * 9.5f, cs * 15.5f, -11.5f + sn * 15.5f, 1.5f, i % 3 ? dark : tip);
                }
                if (curl < 0.9f)
                {
                    Fill(9, -6, 4.5f, 3.4f, face);
                    Fill(13, -6.2f, 1.4f, 1.2f, nose);
                    eye(9.5f, -8.2f, 1.2f, nose);
                }
            }
            else
            {
                float waddle = std::sin(c.anim) * 2;
                T = place(0, waddle, breathe);
                dc->SetTransform(T);
                for (int i = 0; i < 4; i++)
                {
                    float lx = -9 + i * 6 + std::sin(c.anim + i * 1.7f) * 1.5f;
                    float lift = c.state == 0 ? std::max(0.0f, std::sin(c.anim + i * Pi)) * 1.4f : 0;
                    Fill(lx, -1.6f - lift, 2.3f, 1.8f, dark);
                }
                Fill(-1, -9.5f, 16.5f, 9.5f, spine);
                // three rows of spines sweeping backwards, darker at the root, pale at the tips
                for (int row = 0; row < 3; row++)
                {
                    float rx = 16.5f - row * 3.8f, ry = 9.5f - row * 2.6f;
                    for (int i = 0; i <= 13; i++)
                    {
                        float ang = Pi * (1.02f + 0.86f * i / 13) + row * 0.07f;
                        float bx = -1 + std::cos(ang) * rx, by = -9.5f + std::sin(ang) * ry;
                        float ox = std::cos(ang) - 0.7f, oy = std::sin(ang);
                        float l = std::sqrt(ox * ox + oy * oy);
                        float len = 5.0f - row * 0.8f;
                        Stroke(bx, by, bx + ox / l * len, by + oy / l * len, 1.4f, (i + row) % 3 == 0 ? tip : dark);
                    }
                }
                float sniff = c.state == 1 ? std::sin(c.aux) * 0.7f : 0;
                float headLow = c.state == 1 ? 1.2f : 0;
                Fill(13, -6.3f + headLow, 7.5f, 5.2f, face);
                Fill(18.8f + sniff * 0.3f, -5.8f + headLow, 3.4f, 2.6f, face);
                Fill(21.8f + sniff, -6.1f + headLow, 1.9f, 1.6f, nose);
                Fill(21.4f + sniff, -6.6f + headLow, 0.6f, 0.5f, C(1, 1, 1, 0.6f));
                eye(15.3f + lookFwd * 0.4f, -8.4f + headLow, 1.35f, nose);
                Fill(16.5f, -5.0f + headLow, 1.7f, 1.0f, blush);
                Fill(11.5f, -11.2f + headLow, 2.1f, 1.9f, face);
                Fill(11.6f, -11.0f + headLow, 0.9f, 0.8f, C(0.6f, 0.45f, 0.35f));
            }
            // leaves speared on the spines
            static const float spots[4][3] = { { -7, -17, 35 }, { 0, -19.5f, -20 }, { 7, -16.5f, 60 }, { -2, -14, 100 } };
            for (int i = 0; i < c.count && i < 4; i++)
            {
                float lx = spots[i][0] * (curl > 0.5f ? 0.7f : 1.0f), ly = curl > 0.5f ? spots[i][1] - 3 : spots[i][1];
                rotated(spots[i][2], lx, ly);
                D2D1_COLOR_F lc = LeafFlat[c.carried[i] % Sim::LeafColors];
                lc.a = a;
                D2D1_POINT_2F leaf[] = { { lx - 7, ly }, { lx - 3, ly - 2.6f }, { lx + 3, ly - 2.4f }, { lx + 7, ly }, { lx + 3, ly + 2.4f }, { lx - 3, ly + 2.6f } };
                Poly(leaf, 6, lc);
                Stroke(lx - 6, ly, lx + 6, ly, 0.6f, C(lc.r * 0.7f, lc.g * 0.7f, lc.b * 0.7f, 0.7f));
                unrotate();
            }
            break;
        }
        case Caterpillar:
        {
            D2D1_COLOR_F body = C(0.50f, 0.68f, 0.26f), light = C(0.72f, 0.85f, 0.42f), spot = C(0.95f, 0.82f, 0.34f);
            D2D1_COLOR_F head = C(0.40f, 0.56f, 0.18f), legs = C(0.30f, 0.42f, 0.14f), dark = C(0.08f, 0.08f, 0.05f);
            const int n = 9;
            if (c.state == 2)
            {
                // curled into a wobbling ring
                shadow(8);
                float wob = std::sin(c.aux) * 0.8f;
                for (int i = n - 1; i >= 0; i--)
                {
                    float ang = -Pi / 2 + i * 2 * Pi / (n + 1);
                    float px = std::cos(ang) * 6.5f + wob, py = -7.5f + std::sin(ang) * 6.5f;
                    Fill(px, py, 3.3f, 3.3f, i == 0 ? head : body);
                    if (i) Fill(px - 0.6f, py - 1.2f, 1.4f, 0.9f, light);
                }
                break;
            }
            float arch = c.state == 1 ? 0.0f : std::max(0.0f, -std::sin(c.anim));
            float len = 32 * (1 - 0.35f * arch);
            float munch = c.state == 1 ? std::sin(c.anim) * 1.2f : 0;
            shadow(len * 0.55f);
            for (int i = n - 1; i >= 0; i--)
            {
                float u = float(i) / (n - 1);
                float px = -u * len, py = -3.3f - arch * 12 * std::sin(Pi * u);
                if (i == 0) py += munch;
                if (py > -6 && i > 0)
                    Stroke(px, py + 2.4f, px + std::sin(c.anim * 3 + i) * 0.9f, 0, 0.9f, legs);
                Fill(px, py, 3.5f, 3.3f, i == 0 ? head : body);
                if (i > 0)
                {
                    Fill(px - 0.5f, py - 1.4f, 1.6f, 0.9f, light);
                    if (i % 2) Fill(px + 0.4f, py + 0.6f, 0.8f, 0.8f, spot);
                }
            }
            float bob = std::sin(c.t * 4) * 0.8f;
            Stroke(0.4f, -6 + munch, 1.6f, -9.5f + bob + munch, 0.7f, head);
            Stroke(-0.8f, -6 + munch, -0.6f, -9.8f - bob + munch, 0.7f, head);
            Fill(1.6f, -9.5f + bob + munch, 0.8f, 0.8f, head);
            Fill(-0.6f, -9.8f - bob + munch, 0.8f, 0.8f, head);
            eye(1.6f, -4.6f + munch, 0.9f, dark);
            Stroke(2.2f, -2.4f + munch, 3.3f, -2.9f + munch, 0.5f, dark);
            break;
        }
        case SnowFox:
        {
            D2D1_COLOR_F fur = C(0.98f, 0.97f, 0.94f), shade = C(0.76f, 0.79f, 0.86f), cream = C(0.94f, 0.91f, 0.86f);
            D2D1_COLOR_F dark = C(0.17f, 0.14f, 0.14f), pink = C(0.94f, 0.76f, 0.73f);
            bool partly = c.state == 0 || c.state == 3;
            bool flying = c.state == 2;
            float crouch = c.state == 4 ? c.squash : 0;
            float wiggle = crouch * std::sin(c.t * 28) * 1.6f;
            if (!partly && !flying) shadow(20);
            if (partly)
            {
                // only the part above the snow shows while emerging or diving in
                dc->SetTransform(D2D1::Matrix3x2F::Identity());
                dc->PushAxisAlignedClip(D2D1::RectF(-1e5f, -1e5f, 1e5f, sim.SurfaceY(c.x) * k), D2D1_ANTIALIAS_MODE_ALIASED);
                T = place((1 - c.grow) * 42, c.state == 3 ? 70.0f : 0.0f);
            }
            else T = place(crouch * 3, c.angle, breathe - crouch * 0.12f);
            dc->SetTransform(T);
            // every part over a slightly larger grey-blue silhouette, so white fur reads against snow
            auto both = [&](float cx, float cy, float rx, float ry, D2D1_COLOR_F col) { Fill(cx, cy, rx + 1, ry + 1, shade); Fill(cx, cy, rx, ry, col); };
            float tailLift = flying ? 4.0f : c.aux2 * 0.4f;
            both(-20 + wiggle, -16 - tailLift * 0.3f, 9, 6.5f, fur);
            both(-28 + wiggle, -19 - tailLift * 0.6f, 7, 5.5f, fur);
            both(-33 + wiggle, -21 - tailLift, 4.5f, 4, cream);
            for (int i = 0; i < 4; i++)
            {
                float lx = -10 + i * 6.5f;
                float stretch = flying ? (i < 2 ? -7.0f : 7.0f) : 0;
                float ly = flying ? -3.0f : -1.0f;
                Stroke(lx, -10, lx + stretch, ly, 4.4f, shade);
                Stroke(lx, -10, lx + stretch, ly, 3.2f, i < 2 ? cream : fur);
                Fill(lx + stretch, ly + 0.4f, 2.2f, 1.3f, fur);
            }
            both(-3 + wiggle * 0.5f, -14, 15, 8.5f, fur);
            both(8.5f, -13, 6, 6.5f, cream);   // chest fluff
            // the head, cocked while listening
            rotated(c.state == 1 ? c.aux : 0.0f, 11, -18);
            float lx = lookFwd * 0.8f;
            both(10 + lx, -19, 5, 4.5f, cream);   // cheek ruff
            both(13 + lx, -22, 7, 6, fur);
            float twitch = c.aux2 > 0.5f ? 1.5f : 0;
            D2D1_POINT_2F earA[] = { { 9 + lx, -26 }, { 10.5f + lx - twitch, -34.5f }, { 14 + lx, -27 } };
            D2D1_POINT_2F earB[] = { { 14 + lx, -27 }, { 17 + lx, -33.5f }, { 18.5f + lx, -25.5f } };
            Poly(earA, 3, shade); Poly(earB, 3, shade);
            D2D1_POINT_2F innerA[] = { { 10.6f + lx, -27 }, { 11.2f + lx - twitch, -32 }, { 13 + lx, -27.4f } };
            D2D1_POINT_2F innerB[] = { { 15 + lx, -27 }, { 16.8f + lx, -31.5f }, { 17.4f + lx, -26.4f } };
            Poly(innerA, 3, pink); Poly(innerB, 3, pink);
            both(19.5f + lx, -20.6f, 5, 3, fur);   // muzzle
            Fill(24.3f + lx, -21, 1.6f, 1.4f, dark);
            Fill(24 + lx, -21.5f, 0.5f, 0.4f, C(1, 1, 1, 0.6f));
            eye(16 + lx * 1.2f, -23.5f, 1.4f, dark);
            Stroke(20 + lx, -18.6f, 23 + lx, -18.9f, 0.6f, C(0.6f, 0.6f, 0.65f));
            unrotate();
            if (partly)
            {
                dc->SetTransform(D2D1::Matrix3x2F::Identity());
                dc->PopAxisAlignedClip();
            }
            break;
        }
        case Snowdrops:
        {
            D2D1_COLOR_F stem = C(0.38f, 0.56f, 0.26f), leaf = C(0.46f, 0.63f, 0.47f), fold = C(0.36f, 0.52f, 0.38f);
            D2D1_COLOR_F petal = C(0.99f, 0.98f, 0.95f), petalShade = C(0.88f, 0.90f, 0.92f), tip = C(0.50f, 0.70f, 0.30f);
            for (int i = 0; i < c.count; i++)
            {
                float bx = (i - (c.count - 1) / 2.0f) * 7 + std::sin(c.seed + i) * 2;
                float h = (24 + 6 * std::sin(c.seed * 3 + i)) * c.grow;
                float sway = std::sin(c.t * (1.1f + i * 0.17f) + i) * 2;
                float bend = (c.angle + sway + i * 2.5f - (c.count - 1) * 1.2f) * Pi / 180;
                float mx = bx + std::sin(bend * 0.5f) * h * 0.5f, my = -h * 0.5f;
                float tx = bx + std::sin(bend) * h, ty = -std::cos(bend) * h;
                // strap leaves with a fold down the middle
                float lh = h * (0.55f + 0.1f * (i % 2));
                D2D1_POINT_2F blade[] = { { bx - 2, 0 }, { bx - 4 - i % 2 * 2, -lh * 0.6f }, { bx - 3 - i % 2 * 2, -lh }, { bx + 1, 0 } };
                Poly(blade, 4, leaf);
                Stroke(bx - 0.5f, 0, bx - 3 - i % 2 * 2, -lh * 0.95f, 0.5f, fold);
                Stroke(bx, 0, mx, my, 1.4f, stem);
                Stroke(mx, my, tx, ty, 1.3f, stem);
                if (c.grow > 0.5f)
                {
                    // a closed bud first, then the nodding bell opens its three outer petals
                    float f = std::min(1.0f, (c.grow - 0.5f) * 2);
                    float open = std::max(0.0f, (c.grow - 0.8f) * 5);
                    float hx = tx + 3, hy = ty + 1;
                    Stroke(tx, ty, hx, hy, 1.1f, stem);
                    Fill(hx, hy + 0.5f, 1.4f, 1.4f, tip);
                    float spread = 1.2f + open * 1.6f;
                    Fill(hx - spread, hy + 4.6f * f, 1.7f * f, 4.2f * f, petalShade);
                    Fill(hx + spread, hy + 4.6f * f, 1.7f * f, 4.2f * f, petalShade);
                    Fill(hx, hy + 5 * f, 1.9f * f, 4.6f * f, petal);
                    if (open > 0.3f)
                    {
                        Fill(hx, hy + 8.2f, 1.3f * open, 1.0f * open, petal);
                        Fill(hx, hy + 8.6f, 0.8f * open, 0.4f * open, tip);
                    }
                }
            }
            break;
        }
        case Mushrooms:
        {
            bool agaric = c.vx < 0.6f;
            D2D1_COLOR_F stalk = C(0.95f, 0.91f, 0.82f), stalkShade = C(0.85f, 0.80f, 0.70f);
            D2D1_COLOR_F cap = agaric ? C(0.82f, 0.21f, 0.13f) : C(0.54f, 0.33f, 0.17f);
            D2D1_COLOR_F capLight = agaric ? C(0.92f, 0.36f, 0.22f) : C(0.66f, 0.44f, 0.24f);
            D2D1_COLOR_F gills = C(0.88f, 0.82f, 0.70f), grass = C(0.42f, 0.58f, 0.26f);
            for (int g = 0; g < 5; g++)
            {
                float gx = (g - 2) * 7 + std::sin(c.seed + g) * 2;
                Stroke(gx, 0, gx + std::sin(c.t + g) * 1.5f - 1, -4 - (g % 3) * 1.5f, 0.8f, grass);
            }
            for (int i = 0; i < c.count; i++)
            {
                float bx = (i - (c.count - 1) / 2.0f) * 11 + std::sin(c.seed + i * 2) * 2;
                float h = (9 + i * 3.5f + std::sin(c.seed + i) * 2) * c.grow * (1 - c.squash * 0.15f);
                float w = (agaric ? 8.0f : 9.5f) * (0.7f + 0.3f * c.grow) * (1 - i * 0.12f) * (1 + c.squash * 0.1f);
                rotated(c.angle * (1 + i * 0.3f), bx, 0);
                float sw = agaric ? 2.0f : 3.0f;
                D2D1_POINT_2F stem[] = { { bx - sw - (agaric ? 0.8f : 1.5f), 0 }, { bx - sw, -h * 0.4f }, { bx - sw * 0.75f, -h }, { bx + sw * 0.75f, -h },
                                         { bx + sw, -h * 0.4f }, { bx + sw + (agaric ? 0.8f : 1.5f), 0 } };
                Poly(stem, 6, stalk);
                Stroke(bx + sw * 0.4f, -h * 0.1f, bx + sw * 0.3f, -h * 0.9f, 0.8f, stalkShade);
                if (agaric && c.grow > 0.5f) Fill(bx, -h * 0.72f, sw + 1.4f, 1.1f, stalk);   // the little skirt
                D2D1_POINT_2F dome[13];
                for (int j = 0; j <= 12; j++)
                {
                    float ang = Pi + Pi * j / 12;
                    dome[j] = D2D1::Point2F(bx + std::cos(ang) * w, -h + 1 + std::sin(ang) * w * 0.65f * c.grow);
                }
                Poly(dome, 13, cap);
                Fill(bx - w * 0.25f, -h - w * 0.32f * c.grow, w * 0.45f, w * 0.18f * c.grow, capLight);
                Stroke(bx - w * 0.9f, -h + 1.2f, bx + w * 0.9f, -h + 1.2f, 1.0f, gills);
                if (agaric && c.grow > 0.4f)
                {
                    static const float dots[5][3] = { { -0.55f, 0.25f, 1.1f }, { -0.15f, 0.45f, 0.8f }, { 0.25f, 0.3f, 1.2f }, { 0.55f, 0.15f, 0.7f }, { 0.05f, 0.12f, 0.6f } };
                    for (const auto& d : dots) Fill(bx + d[0] * w, -h - d[1] * w * 0.65f * c.grow * 1.3f, d[2], d[2] * 0.85f, C(1, 0.98f, 0.94f));
                }
                unrotate();
            }
            break;
        }
        case Snail:
        {
            // the slime trail lives in screen space and glints as it dries
            dc->SetTransform(D2D1::Matrix3x2F::Identity());
            for (size_t i = 1; i < c.trail.size(); i++)
            {
                float fade = 1 - c.trailAge[i] / 40;
                float glint = 0.25f + 0.2f * std::sin(c.trail[i].x * 0.05f + c.t * 2);
                Stroke(c.trail[i - 1].x * k, c.trail[i - 1].y * k, c.trail[i].x * k, c.trail[i].y * k, 1.6f * S * k * CS * 0.7f, C(0.88f, 0.92f, 0.97f, (glint + 0.15f) * fade));
            }
            dc->SetTransform(T);
            D2D1_COLOR_F body = C(0.78f, 0.70f, 0.58f), bodyDark = C(0.64f, 0.56f, 0.45f), shell = C(0.63f, 0.43f, 0.22f);
            D2D1_COLOR_F band = C(0.80f, 0.60f, 0.34f), line = C(0.42f, 0.27f, 0.13f), eyeCol = C(0.18f, 0.14f, 0.1f);
            float out = c.grow;
            shadow(10);
            if (out > 0.05f)
            {
                // the foot, rippling as it glides
                for (int i = 0; i < 6; i++)
                {
                    float u = i / 5.0f;
                    float px = -4 + u * (9 + 8 * out);
                    Fill(px, -2.2f - std::max(0.0f, std::sin(c.aux - i)) * 0.4f, 3.2f, 2.3f, body);
                }
                for (int i = 0; i < 3; i++) Fill(-1 + i * 4 * out, -1.4f, 0.6f, 0.4f, bodyDark);
                float hx = 6 + 8 * out, wob = std::sin(c.t * 2.3f) * 0.8f;
                Fill(hx, -3.4f, 3, 2.6f, body);
                Stroke(hx + 1, -5, hx + 3 + wob, -5 - 7 * out, 0.9f, body);
                Stroke(hx - 0.5f, -5, hx + 0.5f - wob * 0.5f, -5 - 6 * out, 0.9f, body);
                Fill(hx + 3 + wob, -5 - 7 * out, 1.1f, 1.1f, eyeCol);
                Fill(hx + 0.5f - wob * 0.5f, -5 - 6 * out, 1.0f, 1.0f, eyeCol);
                Stroke(hx + 2.4f, -2.6f, hx + 4, -1.8f, 0.6f, bodyDark);
            }
            // a spiral shell: banded rings drawn inwards with a growth line spiral on top
            Fill(0, -8, 8, 7.5f, shell);
            Fill(0.6f, -8.3f, 5.6f, 5.2f, band);
            Fill(1.0f, -8.5f, 3.6f, 3.4f, shell);
            Fill(1.3f, -8.6f, 1.8f, 1.7f, band);
            for (int i = 0; i < 14; i++)
            {
                float r0 = 7.2f * (1 - i / 15.0f), r1 = 7.2f * (1 - (i + 1) / 15.0f);
                float a0 = i * 0.85f, a1 = (i + 1) * 0.85f;
                Stroke(1 + std::cos(a0) * r0, -8.4f + std::sin(a0) * r0, 1 + std::cos(a1) * r1, -8.4f + std::sin(a1) * r1, 0.8f, line);
            }
            Fill(-2.5f, -12, 2.2f, 1.2f, C(1, 0.95f, 0.85f, 0.35f));   // shine
            break;
        }
        case Frog:
        {
            D2D1_COLOR_F skin = C(0.40f, 0.62f, 0.30f), skinDark = C(0.30f, 0.48f, 0.22f), belly = C(0.84f, 0.87f, 0.60f);
            D2D1_COLOR_F iris = C(0.96f, 0.78f, 0.26f), dark = C(0.10f, 0.10f, 0.06f), tongue = C(0.92f, 0.45f, 0.50f);
            bool jump = c.state == 1;
            if (!jump) shadow(11);
            float sq = c.squash;
            T = place(0, c.angle, (jump ? 1.05f : breathe) * (1 - sq * 0.2f));
            dc->SetTransform(T);
            if (jump)
            {
                Stroke(-5, -4, -16, 1, 3.2f, skin);
                Stroke(-15, 1, -19, 2.2f, 1.6f, skin);
                Stroke(-4, -5, -15, -1, 2.3f, skinDark);
            }
            else
            {
                Fill(-5, -3.5f, 6, 4, skin);
                Fill(-8.5f, -0.8f, 4, 1.4f, skinDark);   // folded foot
            }
            Fill(0, -6.5f, 9.5f, 6.5f, skin);
            Fill(-3, -9, 1.6f, 1.2f, skinDark);
            Fill(-6, -6.5f, 1.3f, 1.0f, skinDark);
            Fill(1, -10, 1.1f, 0.9f, skinDark);
            Fill(2, -4, 6.5f, 3.2f, belly);
            float throat = jump ? 0.0f : std::max(0.0f, std::sin(c.anim)) * 1.3f;
            Fill(7, -3.4f, 2 + throat, 1.8f + throat * 0.6f, belly);
            // front leg with a little webbed hand
            Stroke(6, -3, jump ? 10.0f : 8.0f, 0, 1.7f, skin);
            for (int f = -1; f <= 1; f++) Stroke(jump ? 10.0f : 8.0f, 0, (jump ? 10.0f : 8.0f) + 1.4f + f * 0.4f, 0.2f + f * 0.9f, 0.6f, skin);
            // bulging golden eyes with a horizontal pupil
            Fill(4, -12, 3.3f, 3.3f, skin);
            if (c.blink < 0) Stroke(2.2f, -12.2f, 6, -12.2f, 1.2f, skinDark);
            else
            {
                Fill(4.3f + lookFwd * 0.3f, -12.3f, 2.3f, 2.3f, iris);
                Fill(4.5f + lookFwd * 0.6f, -12.3f, 1.6f, 0.8f, dark);
                Fill(3.6f, -13.1f, 0.6f, 0.6f, C(1, 1, 1, 0.85f));
            }
            Stroke(6.5f, -6.8f, 10.5f, -6.2f, 0.6f, skinDark);
            Fill(9.5f, -8.6f, 0.4f, 0.3f, dark);
            if (c.aux > 0)
            {
                // the tongue shoots out to the snack and back
                float e = std::sin(c.aux / 0.25f * Pi);
                float tx = 10 + (c.vx / CS - 10) * e, ty = -6.5f + (c.vy / CS + 6.5f) * e;
                Stroke(10, -6.5f, tx, ty, 1.1f, tongue);
                Fill(tx, ty, 1.3f, 1.3f, tongue);
            }
            break;
        }
        case Robin:
        {
            D2D1_COLOR_F back = C(0.50f, 0.42f, 0.31f), wing = C(0.42f, 0.35f, 0.26f), feather = C(0.32f, 0.26f, 0.19f);
            D2D1_COLOR_F breast = C(0.92f, 0.48f, 0.22f), belly = C(0.95f, 0.92f, 0.85f), dark = C(0.10f, 0.08f, 0.06f), legs = C(0.40f, 0.28f, 0.22f);
            bool flying = c.state != 1;
            float hop = std::sin(Pi * c.grow) * 6;
            if (!flying) shadow(7);
            T = place(-hop, flying ? 0.0f : 0.0f, (1 - c.squash * 0.15f) * (flying ? 1.0f : breathe));
            dc->SetTransform(T);
            float peck = !flying && c.anim > 0 ? std::sin(c.anim * Pi) * 4 : 0;
            float flick = c.aux2 * 14;
            rotated(-flick, -5, -8);
            D2D1_POINT_2F tail[] = { { -5, -8.5f }, { -14, -12 }, { -13.5f, -9.5f }, { -12, -7 } };
            Poly(tail, 4, wing);
            unrotate();
            if (!flying)
            {
                Stroke(-0.5f, -2, -1, 0, 0.8f, legs);
                Stroke(1.5f, -2, 2, 0, 0.8f, legs);
                Stroke(-1, 0, 1, 0, 0.6f, legs);
                Stroke(2, 0, 4, 0, 0.6f, legs);
            }
            Fill(0, -7, 7.2f, 6.2f, back);
            Fill(2, -3.6f, 4.8f, 2.8f, belly);
            Fill(3.6f, -7.4f, 4.6f, 4.4f, breast);
            // the head, turned towards the pointer when it is around, ducking to peck
            float hx = 5.5f + lookFwd * 0.6f, hy = -12.5f + peck;
            Fill(hx, hy, 4.4f, 4.1f, back);
            Fill(hx + 1.6f, hy + 1.6f, 3, 2.7f, breast);
            eye(hx + 1.2f, hy - 0.4f, 0.95f, dark);
            D2D1_POINT_2F beak[] = { { hx + 3.2f, hy - 0.6f }, { hx + 6.4f, hy + 0.3f }, { hx + 3.2f, hy + 1.1f } };
            Poly(beak, 3, dark);
            if (flying)
            {
                float s = std::sin(c.anim);
                D2D1_POINT_2F far_[] = { { -3, -9 }, { 3, -9 }, { -1, -9 - 9 * s } };
                D2D1_POINT_2F near_[] = { { -4, -8 }, { 3, -8 }, { -3, -8 - 10 * s } };
                Poly(far_, 3, feather);
                Poly(near_, 3, wing);
            }
            else
            {
                Fill(-1.8f, -8, 4.8f, 3.2f, wing);
                for (int f = 0; f < 3; f++) Stroke(-5 + f * 1.6f, -7.5f, -2 + f * 1.6f, -6, 0.5f, feather);
            }
            break;
        }
        case Squirrel:
        {
            D2D1_COLOR_F fur = C(0.76f, 0.42f, 0.18f), furDark = C(0.62f, 0.33f, 0.14f), tailLight = C(0.86f, 0.56f, 0.28f);
            D2D1_COLOR_F belly = C(0.96f, 0.88f, 0.74f), dark = C(0.13f, 0.09f, 0.06f), acorn = C(0.60f, 0.40f, 0.20f), cup = C(0.38f, 0.26f, 0.14f);
            bool sitting = c.state == 1 || c.state == 2;
            float bounce = sitting ? 0.0f : -std::fabs(std::sin(c.anim)) * 10;
            float stretch = sitting ? 1.0f : 1 + std::sin(c.anim * 2) * 0.08f;
            shadow(sitting ? 9.0f : 12.0f);
            T = place(bounce, sitting ? -25.0f : std::sin(c.anim) * 8, sitting ? breathe : 1.0f);
            dc->SetTransform(T);
            // a big S-shaped bushy tail, pale at the rim
            float flick = c.aux2 * 6 + (sitting ? 0.0f : std::sin(c.anim) * 3);
            rotated(flick, -9, -10);
            static const float tail[5][4] = { { -10, -12, 5.5f, 7 }, { -13, -19, 6.5f, 6 }, { -12, -26, 6, 5 }, { -8, -30, 4.5f, 3.6f }, { -5, -31, 2.6f, 2.2f } };
            for (const auto& t : tail) Fill(t[0], t[1], t[2] + 1.2f, t[3] + 1.2f, tailLight);
            for (const auto& t : tail) Fill(t[0], t[1], t[2], t[3], fur);
            unrotate();
            float s = std::sin(c.anim);
            Stroke(-5, -6, -8 - s * 5 * !sitting, 0, 2.8f, furDark);
            Stroke(5, -7, 8 + s * 5 * !sitting, 0, 2.2f, furDark);
            Fill(0, -9, 9.5f * stretch, 6, fur);
            Fill(2.5f, -6.8f, 5, 3, belly);
            float nibble = c.state == 1 ? std::sin(c.aux) * 0.8f : 0;
            float hx = 8 + lookFwd * 0.6f, hy = -13 + nibble * 0.4f;
            Fill(hx, hy, 4.8f, 4.2f, fur);
            Fill(hx + 2.4f, hy + 1.6f, 2.4f, 1.8f, belly);
            D2D1_POINT_2F ear[] = { { hx - 1, hy - 3.5f }, { hx + 0.2f, hy - 8.5f }, { hx + 2.6f, hy - 3.6f } };
            Poly(ear, 3, fur);
            Stroke(hx + 0.2f, hy - 8.5f, hx - 0.6f, hy - 10, 0.8f, furDark);   // ear tuft
            eye(hx + 1.4f, hy - 0.8f, 1.1f, dark);
            Fill(hx + 4.6f, hy + 0.8f, 0.7f, 0.6f, dark);
            // the acorn, held in the paws when sitting up
            float ax = sitting ? hx + 3.8f : hx + 5, ay = sitting ? hy + 4.5f + nibble : hy + 2;
            if (sitting) Stroke(4, -9, ax - 0.5f, ay + 1, 1.6f, furDark);
            Fill(ax, ay, 2, 2.4f, acorn);
            Fill(ax, ay - 1.8f, 2.3f, 1.2f, cup);
            break;
        }
        }
    }
    dc->SetTransform(D2D1::Matrix3x2F::Identity());
}
