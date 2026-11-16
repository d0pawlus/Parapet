#include "app.h"
#include <algorithm>

// Leaf outlines are generated from a handful of botanical parameters, then painted once into the atlas.
// Three families: simple pinnate blades (profile along the midrib), palmate leaves (lobes around a centre)
// and fans (ginkgo).

namespace
{
    const float Pi = 3.14159265f;

    enum Family { Simple, Palmate, Fan };

    struct Spec
    {
        Family family;
        // simple blades
        float width;       // half width relative to length
        float skew;        // >1 moves the widest point towards the base, <1 towards the tip
        float fullness;    // <1 rounder, >1 slimmer shoulders
        float cordate;     // width kept at the base (heart shape), 0 = tapering base
        float notch;       // depth of the notch at a heart-shaped base
        float acuminate;   // drawn-out drip tip
        float retuse;      // blunt notched tip
        float asym;        // left half wider towards the base (elm, linden)
        int lobes; float lobeDepth, lobeSharp;
        int teeth; float tooth, doubleTooth;
        // palmate / fan
        int count; float spread, rmin, sigma; const float* lengths; float truncate;
        float lobePow;     // palmate lobe profile: 1 = pointed tips, 2 = rounded lobes
    };

    const float Maple5[] = { 0.62f, 0.92f, 1.0f, 0.92f, 0.62f };
    const float Japanese7[] = { 0.55f, 0.8f, 0.95f, 1.0f, 0.95f, 0.8f, 0.55f };
    const float Even5[] = { 0.8f, 0.95f, 1.0f, 0.95f, 0.8f };
    const float Grape3[] = { 0.8f, 1.0f, 0.8f };
    const float Mitten[] = { 1.0f, 0.75f };
    const float Tulip4[] = { 0.75f, 1.0f, 1.0f, 0.75f };
    const float Hawthorn5[] = { 0.7f, 0.9f, 1.0f, 0.9f, 0.7f };

    Spec S(float width, float skew, float fullness)
    {
        Spec s = {};
        s.family = Simple;
        s.width = width; s.skew = skew; s.fullness = fullness;
        return s;
    }
    Spec Teeth(Spec s, int n, float amp, float dbl = 0) { s.teeth = n; s.tooth = amp; s.doubleTooth = dbl; return s; }
    Spec Lobes(Spec s, int n, float depth, float sharp) { s.lobes = n; s.lobeDepth = depth; s.lobeSharp = sharp; return s; }
    Spec Heart(Spec s, float base, float notch) { s.cordate = base; s.notch = notch; return s; }
    Spec Tip(Spec s, float acuminate, float retuse = 0) { s.acuminate = acuminate; s.retuse = retuse; return s; }
    Spec Asym(Spec s, float a) { s.asym = a; return s; }
    Spec P(int count, float spread, float rmin, float sigma, const float* lengths, int teeth, float tooth, float lobePow = 1.2f)
    {
        Spec s = {};
        s.family = Palmate;
        s.count = count; s.spread = spread; s.rmin = rmin; s.sigma = sigma; s.lengths = lengths;
        s.teeth = teeth; s.tooth = tooth; s.lobePow = lobePow;
        return s;
    }

    Spec Species(int i)
    {
        switch (i)
        {
        case 0:  return Teeth(S(0.36f, 1.1f, 0.8f), 18, 0.012f);                                    // beech
        case 1:  return P(5, 1.40f * Pi, 0.44f, 0.30f, Maple5, 26, 0.11f, 1.1f);                   // Norway maple
        case 2:  return Teeth(Tip(S(0.12f, 1.0f, 0.9f), 0.5f), 60, 0.006f);                         // willow
        case 3:  return Lobes(S(0.34f, 1.2f, 0.7f), 4, 0.5f, 0.6f);                                 // English oak
        case 4:  return Teeth(Lobes(S(0.40f, 1.1f, 0.6f), 3, 0.66f, 0.35f), 10, 0.02f);            // red oak
        case 5:  return Teeth(Tip(S(0.36f, 1.6f, 0.65f), 0.4f), 34, 0.02f, 0.5f);                  // birch
        case 6:  return Asym(Teeth(Tip(Heart(S(0.44f, 1.35f, 0.6f), 0.45f, 0.08f), 0.5f), 40, 0.014f), 0.18f); // linden
        case 7:  return Asym(Teeth(Tip(S(0.34f, 1.2f, 0.75f), 0.3f), 30, 0.02f, 0.6f), 0.22f);     // elm
        case 8:  return Teeth(Heart(S(0.48f, 1.25f, 0.45f), 0.3f, 0.03f), 16, 0.035f);              // aspen
        case 9:  return P(5, 1.50f * Pi, 0.40f, 0.25f, Even5, 0, 0, 1.0f);                         // sweetgum
        case 10: return P(5, 1.38f * Pi, 0.46f, 0.32f, Maple5, 14, 0.14f, 1.1f);                   // sugar maple
        case 11: return P(7, 1.60f * Pi, 0.28f, 0.17f, Japanese7, 70, 0.035f, 1.1f);               // Japanese maple
        case 12: return P(5, 1.25f * Pi, 0.55f, 0.34f, Even5, 14, 0.07f, 1.2f);                    // plane tree
        case 13: { Spec s = {}; s.family = Fan; s.spread = 0.78f * Pi; return s; }                // ginkgo
        case 14: return Teeth(Tip(S(0.30f, 1.15f, 0.8f), 0.45f), 46, 0.016f, 0.7f);                // hornbeam
        case 15: return Teeth(Tip(S(0.18f, 1.0f, 0.85f), 0.4f), 22, 0.05f);                        // sweet chestnut
        case 16: return Teeth(S(0.30f, 0.62f, 0.9f), 34, 0.018f);                                  // horse chestnut leaflet
        case 17: return Teeth(Tip(S(0.26f, 1.0f, 0.75f), 0.7f), 50, 0.01f);                        // cherry
        case 18: return Teeth(Tip(S(0.42f, 0.75f, 0.6f), 0, 0.12f), 26, 0.022f, 0.5f);            // alder
        case 19: return Teeth(Tip(Heart(S(0.48f, 1.1f, 0.5f), 0.4f, 0.06f), 0.55f), 30, 0.03f, 0.8f); // hazel
        case 20: return Teeth(S(0.15f, 1.0f, 0.5f), 34, 0.018f);                                   // rowan leaflet
        case 21: return P(5, 1.30f * Pi, 0.55f, 0.36f, Maple5, 0, 0, 2.0f);                        // field maple
        case 22: return P(2, 0.55f * Pi, 0.50f, 0.45f, Mitten, 0, 0, 1.3f);                        // sassafras mitten
        case 23: return P(3, 0.75f * Pi, 0.70f, 0.50f, Grape3, 30, 0.07f, 1.5f);                   // grape vine
        case 24: return Teeth(Tip(S(0.17f, 1.0f, 0.8f), 0.55f), 28, 0.012f);                       // ash leaflet
        case 25: return S(0.30f, 0.9f, 0.7f);                                                      // magnolia
        case 26: return P(5, 1.20f * Pi, 0.34f, 0.22f, Hawthorn5, 18, 0.06f, 1.2f);                // hawthorn
        default: return Teeth(Tip(Heart(S(0.46f, 1.2f, 0.5f), 0.5f, 0.07f), 0.35f), 42, 0.025f);   // mulberry
        }
    }

    float Smoothstep(float a, float b, float x)
    {
        float t = std::max(0.0f, std::min(1.0f, (x - a) / (b - a)));
        return t * t * (3 - 2 * t);
    }

    float Frac(float x) { return x - std::floor(x); }

    // half width of a simple blade at t (0 = tip, 1 = base)
    float Profile(const Spec& s, float t)
    {
        float prof = std::pow(std::max(0.0f, std::sin(Pi * std::pow(t, s.skew))), s.fullness);
        if (s.cordate > 0) prof = std::max(prof, s.cordate * Smoothstep(0.45f, 1.0f, t));
        if (s.acuminate > 0) prof *= 1 - s.acuminate + s.acuminate * Smoothstep(0.0f, 0.32f, t);
        if (s.retuse > 0) prof += s.retuse * std::max(0.0f, 1 - t / 0.14f) * Smoothstep(0.0f, 0.03f, t);
        if (s.lobes > 0)
        {
            float u = (t - 0.06f) / 0.86f;
            if (u > 0 && u < 1)
            {
                // lobe tips at u = k / lobes; lobeSharp < 1 gives round lobes, > 1 narrow pointed ones
                float bulge = std::pow(std::fabs(std::cos(Pi * s.lobes * u)), s.lobeSharp);
                float fade = Smoothstep(0.0f, 0.1f, u) * (1 - Smoothstep(0.85f, 1.0f, u));
                prof *= 1 - s.lobeDepth * (1 - bulge) * fade;
            }
        }
        return prof;
    }

    float ToothOffset(const Spec& s, float phase)
    {
        if (s.teeth <= 0) return 0;
        // saw teeth leaning towards the tip, optionally with a smaller tooth on each flank
        float saw = 1 - Frac(phase);
        float off = s.tooth * saw * saw;
        if (s.doubleTooth > 0) off += s.tooth * s.doubleTooth * 0.5f * (1 - Frac(phase * 2));
        return off;
    }

    void BuildSimple(const Spec& s, LeafShape& out)
    {
        const int N = 110;
        std::vector<D2D1_POINT_2F> right, left;
        for (int i = 0; i <= N; i++)
        {
            float t = float(i) / N;
            float y = -0.5f + t;
            float w = s.width * Profile(s, t);
            float fade = std::min(1.0f, w / (s.width * 0.25f + 1e-4f));
            float edge = ToothOffset(s, t * s.teeth) * fade;
            float x = w + edge;
            right.push_back(D2D1::Point2F(x, y));
            left.push_back(D2D1::Point2F(-x * (1 + s.asym * t * t), y + s.asym * 0.04f * t));
        }
        out.outline = right;
        if (s.cordate > 0) out.outline.push_back(D2D1::Point2F(0, 0.5f - s.notch));
        for (int i = N; i >= 0; i--) out.outline.push_back(left[i]);

        float base = s.cordate > 0 ? 0.5f - s.notch : 0.5f;
        out.veinFrom.push_back(D2D1::Point2F(0, base));
        out.veinTo.push_back(D2D1::Point2F(0, -0.47f));
        out.stem = D2D1::Point2F(0, base);

        // side veins: to the lobe tips for lobed blades, evenly spaced otherwise
        int pairs = s.lobes > 0 ? s.lobes : std::max(4, std::min(9, int(s.width * 22)));
        for (int k = 0; k < pairs; k++)
        {
            float tt = s.lobes > 0 ? 0.06f + 0.86f * (k + 0.02f) / s.lobes + 0.86f / s.lobes : 0.2f + 0.68f * (k + 0.5f) / pairs;
            if (tt >= 0.97f) continue;
            float tEnd = std::max(0.03f, tt - 0.12f);
            float xEnd = 0.82f * s.width * Profile(s, tEnd);
            out.veinFrom.push_back(D2D1::Point2F(0, -0.5f + tt));
            out.veinTo.push_back(D2D1::Point2F(xEnd, -0.5f + tEnd));
            out.veinFrom.push_back(D2D1::Point2F(0, -0.5f + tt));
            out.veinTo.push_back(D2D1::Point2F(-xEnd * (1 + s.asym * tEnd * tEnd), -0.5f + tEnd));
        }
    }

    void BuildPalmate(const Spec& s, LeafShape& out)
    {
        const int N = 260;
        const D2D1_POINT_2F c = D2D1::Point2F(0, 0.12f);
        std::vector<float> centres(s.count);
        for (int k = 0; k < s.count; k++)
            centres[k] = s.count == 1 ? 0 : -s.spread / 2 + s.spread * k / (s.count - 1);
        if (s.count == 2) { centres[0] = -0.08f * Pi; centres[1] = 0.47f * Pi; }   // mitten: main lobe and a thumb

        auto radius = [&](float phi, float& lobe) {
            lobe = 0;
            for (int k = 0; k < s.count; k++)
            {
                float d = phi - centres[k];
                lobe = std::max(lobe, s.lengths[k] * std::exp(-std::pow(std::fabs(d) / s.sigma, s.lobePow)));
            }
            float r = s.rmin + (1 - s.rmin) * lobe;
            // around the petiole the blade pulls in to a small notch
            float back = std::fabs(std::fabs(phi) - Pi);
            r *= 0.55f + 0.45f * Smoothstep(0.0f, 0.5f, back);
            r += ToothOffset(s, (phi + Pi) / (2 * Pi) * s.teeth) * Smoothstep(0.05f, 0.4f, lobe);
            return r;
        };

        float lobe;
        for (int i = 0; i < N; i++)
        {
            float phi = -Pi + 2 * Pi * (i + 0.5f) / N;
            float r = radius(phi, lobe) * 0.5f;
            out.outline.push_back(D2D1::Point2F(c.x + r * std::sin(phi), c.y - r * std::cos(phi)));
        }
        for (int k = 0; k < s.count; k++)
        {
            float r = radius(centres[k], lobe) * 0.5f * 0.88f;
            out.veinFrom.push_back(c);
            out.veinTo.push_back(D2D1::Point2F(c.x + r * std::sin(centres[k]), c.y - r * std::cos(centres[k])));
        }
        out.stem = D2D1::Point2F(c.x, c.y + 0.5f * s.rmin * 0.55f);
    }

    void BuildFan(const Spec& s, LeafShape& out)
    {
        const int N = 90;
        const D2D1_POINT_2F apex = D2D1::Point2F(0, 0.42f);
        out.outline.push_back(apex);
        for (int i = 0; i <= N; i++)
        {
            float phi = -s.spread / 2 + s.spread * i / N;
            float r = 0.86f * (1 + 0.035f * std::sin(phi * 23)) * (1 - 0.32f * std::exp(-(phi * phi) / 0.006f));
            out.outline.push_back(D2D1::Point2F(apex.x + r * std::sin(phi), apex.y - r * std::cos(phi)));
        }
        for (int k = 0; k < 11; k++)
        {
            float phi = -s.spread * 0.42f + s.spread * 0.84f * k / 10;
            out.veinFrom.push_back(apex);
            out.veinTo.push_back(D2D1::Point2F(apex.x + 0.78f * std::sin(phi), apex.y - 0.78f * std::cos(phi)));
        }
        out.stem = apex;
    }
}

void BuildLeafShape(int species, LeafShape& out)
{
    out = LeafShape();
    Spec s = Species(species);
    if (s.family == Simple) BuildSimple(s, out);
    else if (s.family == Palmate) BuildPalmate(s, out);
    else BuildFan(s, out);

    // normalise: centre the outline and make its larger extent exactly 1 (fits [-0.5, 0.5], stem below)
    float x0 = 1e9f, y0 = 1e9f, x1 = -1e9f, y1 = -1e9f;
    for (const D2D1_POINT_2F& p : out.outline)
    {
        x0 = std::min(x0, p.x); x1 = std::max(x1, p.x);
        y0 = std::min(y0, p.y); y1 = std::max(y1, p.y);
    }
    float scale = 0.92f / std::max(x1 - x0, y1 - y0 + 0.08f);
    float cx = (x0 + x1) / 2, cy = (y0 + y1) / 2 - 0.04f;
    auto fit = [&](D2D1_POINT_2F& p) { p.x = (p.x - cx) * scale; p.y = (p.y - cy) * scale; };
    for (D2D1_POINT_2F& p : out.outline) fit(p);
    for (D2D1_POINT_2F& p : out.veinFrom) fit(p);
    for (D2D1_POINT_2F& p : out.veinTo) fit(p);
    fit(out.stem);
}
