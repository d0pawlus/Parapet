#include "app.h"
#include <algorithm>

// All particles are drawn as sprites from one atlas in a single DrawSpriteBatch call;
// the GPU does the rasterising and DirectComposition blends the result over the desktop.

namespace
{
    // Atlas: every leaf species x colour, painted at 4 mip sizes (each level packed below the previous),
    // so any on-screen size samples a prefiltered cell instead of shimmering. Then a soft dot, a ripple
    // ring and a rain streak on the right.
    const UINT Base = 96, Levels = 4, LeafAreaW = 16 * Base;
    const UINT LeafCells = Sim::LeafShapes * Sim::LeafColors;
    const UINT DotX = LeafAreaW, DotSize = 64, RingY = DotSize, FlakeY = 2 * DotSize, SoftY = 3 * DotSize, SoftW = 16, SoftFade = 8, SoftH = 32, StreakX = DotX + DotSize, StreakW = 16, StreakH = 128;
    const UINT AtlasW = StreakX + StreakW;
    const float Pi = 3.14159265f;

    UINT LevelY(UINT level)
    {
        UINT y = 0;
        for (UINT l = 0; l < level; l++)
        {
            UINT c = Base >> l, cols = LeafAreaW / c;
            y += (LeafCells + cols - 1) / cols * c;
        }
        return y;
    }

    D2D1_RECT_U LeafCell(UINT shape, UINT color, UINT level)
    {
        UINT c = Base >> level, cols = LeafAreaW / c, i = shape * Sim::LeafColors + color;
        UINT x = i % cols * c, y = LevelY(level) + i / cols * c;
        return D2D1::RectU(x, y, x + c, y + c);
    }

    // flat, warm autumn colours: no gradients, it should feel cosy rather than glossy
    const D2D1_COLOR_F Palettes[Sim::LeafColors] = {
        { 0.77f, 0.28f, 0.12f, 1 }, { 0.87f, 0.47f, 0.12f, 1 }, { 0.92f, 0.67f, 0.16f, 1 }, { 0.63f, 0.16f, 0.12f, 1 },
        { 0.80f, 0.59f, 0.24f, 1 }, { 0.55f, 0.35f, 0.17f, 1 }, { 0.90f, 0.75f, 0.27f, 1 }, { 0.71f, 0.39f, 0.16f, 1 },
    };

    D2D1_COLOR_F Shade(D2D1_COLOR_F c, float f, float a = 1)
    {
        return D2D1::ColorF(std::min(1.0f, c.r * f), std::min(1.0f, c.g * f), std::min(1.0f, c.b * f), a);
    }

    D2D1_MATRIX_3X2_F Place(float sx, float sy, float degrees, float x, float y)
    {
        float a = degrees * (Pi / 180.0f), c = std::cos(a), s = std::sin(a);
        D2D1_MATRIX_3X2_F m;
        m._11 = sx * c;  m._12 = sx * s;
        m._21 = -sy * s; m._22 = sy * c;
        m._31 = x;       m._32 = y;
        return m;
    }
}

bool Renderer::Init(HWND hwnd, int width, int height, float renderScale, bool leafShadows)
{
    k = renderScale;
    shadows = leafShadows;
    UINT bw = std::max(1u, UINT(width * k + 0.5f)), bh = std::max(1u, UINT(height * k + 0.5f));

    HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   nullptr, 0, D3D11_SDK_VERSION, &d3d, nullptr, nullptr);
    if (FAILED(hr))
        hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                               nullptr, 0, D3D11_SDK_VERSION, &d3d, nullptr, nullptr);
    if (FAILED(hr)) return false;

    ComPtr<IDXGIDevice> dxgiDevice;
    ComPtr<IDXGIFactory2> dxgiFactory;
    if (FAILED(d3d.As(&dxgiDevice))) return false;
    // queue at most one frame ahead: Present then paces the loop to the display without adding lag
    ComPtr<IDXGIDevice1> dxgiDevice1;
    if (SUCCEEDED(d3d.As(&dxgiDevice1))) dxgiDevice1->SetMaximumFrameLatency(1);
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&dxgiFactory)))) return false;

    DXGI_SWAP_CHAIN_DESC1 sd = {};
    sd.Width = bw;
    sd.Height = bh;
    sd.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.BufferCount = 2;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
    sd.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
    if (FAILED(dxgiFactory->CreateSwapChainForComposition(dxgiDevice.Get(), &sd, nullptr, &swap))) return false;

    D2D1_FACTORY_OPTIONS fo = {};
    if (FAILED(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, __uuidof(ID2D1Factory1), &fo, &factory))) return false;
    if (FAILED(factory->CreateDevice(dxgiDevice.Get(), &d2dDevice))) return false;
    ComPtr<ID2D1DeviceContext> ctx;
    if (FAILED(d2dDevice->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &ctx))) return false;
    if (FAILED(ctx.As(&dc))) return false;   // needs Windows 10 for sprite batches

    ComPtr<IDXGISurface> surface;
    if (FAILED(swap->GetBuffer(0, IID_PPV_ARGS(&surface)))) return false;
    D2D1_BITMAP_PROPERTIES1 bp = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    if (FAILED(dc->CreateBitmapFromDxgiSurface(surface.Get(), &bp, &target))) return false;

    if (!BuildAtlas()) return false;
    if (FAILED(dc->CreateSpriteBatch(&batch))) return false;
    if (FAILED(dc->CreateSpriteBatch(&layerBatch))) return false;
    dc->CreateSolidColorBrush(D2D1::ColorF(0.85f, 0.9f, 1.0f, 0.0f), &flashBrush);
    dc->SetTarget(target.Get());

    if (FAILED(DCompositionCreateDevice(dxgiDevice.Get(), IID_PPV_ARGS(&dcomp)))) return false;
    if (FAILED(dcomp->CreateTargetForHwnd(hwnd, TRUE, &dcompTarget))) return false;
    if (FAILED(dcomp->CreateVisual(&visual))) return false;
    visual->SetContent(swap.Get());
    if (k != 1.0f)
    {
        visual->SetTransform(D2D1::Matrix3x2F::Scale(1.0f / k, 1.0f / k));
        visual->SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR);
    }
    dcompTarget->SetRoot(visual.Get());
    return SUCCEEDED(dcomp->Commit()) && Clear();
}

void Renderer::Release()
{
    visual.Reset(); dcompTarget.Reset(); dcomp.Reset();
    flashBrush.Reset(); paint.Reset(); layerBatch.Reset();
    batch.Reset(); atlas.Reset(); target.Reset();
    dc.Reset(); d2dDevice.Reset(); factory.Reset();
    swap.Reset(); d3d.Reset();
}

bool Renderer::BuildAtlas()
{
    UINT atlasH = std::max(LevelY(Levels), StreakH);
    D2D1_BITMAP_PROPERTIES1 bp = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    if (FAILED(dc->CreateBitmap(D2D1::SizeU(AtlasW, atlasH), nullptr, 0, &bp, &atlas))) return false;

    LeafShape shapes[Sim::LeafShapes];
    ComPtr<ID2D1PathGeometry> geos[Sim::LeafShapes];
    for (int s = 0; s < Sim::LeafShapes; s++)
    {
        BuildLeafShape(s, shapes[s]);
        ComPtr<ID2D1GeometrySink> sink;
        factory->CreatePathGeometry(&geos[s]);
        geos[s]->Open(&sink);
        sink->BeginFigure(shapes[s].outline[0], D2D1_FIGURE_BEGIN_FILLED);
        sink->AddLines(shapes[s].outline.data() + 1, UINT32(shapes[s].outline.size() - 1));
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        sink->Close();
    }

    dc->SetTarget(atlas.Get());
    dc->BeginDraw();
    dc->Clear(D2D1::ColorF(0, 0, 0, 0));
    dc->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

    ComPtr<ID2D1SolidColorBrush> solid;
    dc->CreateSolidColorBrush(D2D1::ColorF(0, 0, 0, 1), &solid);

    // every leaf is painted directly at each mip size, so small leaves stay crisp instead of being shrunk
    for (UINT level = 0; level < Levels; level++)
    {
        float cell = float(Base >> level), unit = cell * 0.92f, px = 1.0f / unit;
        for (int s = 0; s < Sim::LeafShapes; s++)
        {
            const LeafShape& ls = shapes[s];
            for (int c = 0; c < Sim::LeafColors; c++)
            {
                D2D1_RECT_U r = LeafCell(s, c, level);
                dc->SetTransform(D2D1::Matrix3x2F::Scale(unit, unit) *
                                 D2D1::Matrix3x2F::Translation(r.left + cell / 2.0f, r.top + cell / 2.0f));
                const D2D1_COLOR_F& body = Palettes[c];
                solid->SetColor(body);
                dc->FillGeometry(geos[s].Get(), solid.Get());

                // soft veins in a slightly deeper shade of the same colour (the first one is the midrib), then the stem
                float rib = std::max(px * 1.1f, 0.02f);
                solid->SetColor(Shade(body, 0.72f, 0.55f));
                for (size_t v = 0; v < ls.veinFrom.size(); v++)
                    dc->DrawLine(ls.veinFrom[v], ls.veinTo[v], solid.Get(), v == 0 ? rib : rib * 0.6f);
                solid->SetColor(Shade(body, 0.6f, 1.0f));
                dc->DrawLine(ls.stem, D2D1::Point2F(ls.stem.x + 0.015f, std::min(0.53f, ls.stem.y + 0.12f)), solid.Get(), std::max(px * 1.4f, 0.028f));
            }
        }
    }
    dc->SetTransform(D2D1::Matrix3x2F::Identity());

    // soft dot for snowflakes and splashes
    D2D1_GRADIENT_STOP dotStops[] = { { 0.0f, D2D1::ColorF(1, 1, 1, 1) }, { 0.45f, D2D1::ColorF(1, 1, 1, 0.9f) }, { 1.0f, D2D1::ColorF(1, 1, 1, 0) } };
    ComPtr<ID2D1GradientStopCollection> dotCol;
    ComPtr<ID2D1RadialGradientBrush> dot;
    dc->CreateGradientStopCollection(dotStops, 3, &dotCol);
    D2D1_POINT_2F dc0 = D2D1::Point2F(DotX + DotSize / 2.0f, DotSize / 2.0f);
    dc->CreateRadialGradientBrush(D2D1::RadialGradientBrushProperties(dc0, D2D1::Point2F(0, 0), DotSize / 2.0f - 2, DotSize / 2.0f - 2), dotCol.Get(), &dot);
    dc->FillEllipse(D2D1::Ellipse(dc0, DotSize / 2.0f - 2, DotSize / 2.0f - 2), dot.Get());

    // ripple ring: a bright line with a faint halo
    D2D1_POINT_2F rc = D2D1::Point2F(DotX + DotSize / 2.0f, RingY + DotSize / 2.0f);
    solid->SetColor(D2D1::ColorF(1, 1, 1, 0.35f));
    dc->DrawEllipse(D2D1::Ellipse(rc, 25, 25), solid.Get(), 7);
    solid->SetColor(D2D1::ColorF(1, 1, 1, 1));
    dc->DrawEllipse(D2D1::Ellipse(rc, 25, 25), solid.Get(), 2.5f);

    // snowflake: a plain flat disc with just a soft rim; cosy rather than glowing
    D2D1_GRADIENT_STOP flakeStops[] = { { 0.0f, D2D1::ColorF(1, 1, 1, 1) }, { 0.78f, D2D1::ColorF(1, 1, 1, 1) }, { 1.0f, D2D1::ColorF(1, 1, 1, 0) } };
    ComPtr<ID2D1GradientStopCollection> flakeCol;
    ComPtr<ID2D1RadialGradientBrush> flake;
    dc->CreateGradientStopCollection(flakeStops, 3, &flakeCol);
    D2D1_POINT_2F fc = D2D1::Point2F(DotX + DotSize / 2.0f, FlakeY + DotSize / 2.0f);
    dc->CreateRadialGradientBrush(D2D1::RadialGradientBrushProperties(fc, D2D1::Point2F(0, 0), 28, 28), flakeCol.Get(), &flake);
    dc->FillEllipse(D2D1::Ellipse(fc, 28, 28), flake.Get());

    // soft-topped bar for the surface layers: transparent at the top, ramping to solid over SoftFade texels,
    // so their sloping edges come out smooth even though sprites are drawn without antialiasing
    D2D1_GRADIENT_STOP softStops[] = { { 0.0f, D2D1::ColorF(1, 1, 1, 0) }, { 1.0f, D2D1::ColorF(1, 1, 1, 1) } };
    ComPtr<ID2D1GradientStopCollection> softCol;
    ComPtr<ID2D1LinearGradientBrush> soft;
    dc->CreateGradientStopCollection(softStops, 2, &softCol);
    dc->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(D2D1::Point2F(0, float(SoftY)), D2D1::Point2F(0, float(SoftY + SoftFade))), softCol.Get(), &soft);
    dc->FillRectangle(D2D1::RectF(float(DotX), float(SoftY), float(DotX + SoftW), float(SoftY + SoftFade)), soft.Get());
    solid->SetColor(D2D1::ColorF(1, 1, 1, 1));
    dc->FillRectangle(D2D1::RectF(float(DotX), float(SoftY + SoftFade), float(DotX + SoftW), float(SoftY + SoftH)), solid.Get());

    // rain streak, fading towards its tail
    D2D1_GRADIENT_STOP streakStops[] = { { 0.0f, D2D1::ColorF(1, 1, 1, 0) }, { 1.0f, D2D1::ColorF(1, 1, 1, 1) } };
    ComPtr<ID2D1GradientStopCollection> streakCol;
    ComPtr<ID2D1LinearGradientBrush> streak;
    dc->CreateGradientStopCollection(streakStops, 2, &streakCol);
    dc->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(D2D1::Point2F(0, 4), D2D1::Point2F(0, StreakH - 4.0f)), streakCol.Get(), &streak);
    dc->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(StreakX + 5.0f, 4, StreakX + StreakW - 5.0f, StreakH - 4.0f), 3, 3), streak.Get());

    HRESULT hr = dc->EndDraw();
    dc->SetTarget(target.Get());
#ifdef OVERLAY_DEBUG
    // write the full-size leaf cells next to the exe for inspection
    {
        UINT w = LeafAreaW, h = LevelY(1);
        ComPtr<ID2D1Bitmap1> cpu;
        D2D1_BITMAP_PROPERTIES1 rp = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
        D2D1_POINT_2U origin = D2D1::Point2U(0, 0);
        D2D1_RECT_U area = D2D1::RectU(0, 0, w, h);
        D2D1_MAPPED_RECT mapped;
        if (SUCCEEDED(dc->CreateBitmap(D2D1::SizeU(w, h), nullptr, 0, &rp, &cpu)) &&
            SUCCEEDED(cpu->CopyFromBitmap(&origin, atlas.Get(), &area)) && SUCCEEDED(cpu->Map(D2D1_MAP_OPTIONS_READ, &mapped)))
        {
            std::wstring path = Settings::Path() + L".atlas.bmp";
            FILE* f = nullptr;
            if (_wfopen_s(&f, path.c_str(), L"wb") == 0 && f)
            {
                BITMAPFILEHEADER fh = {};
                BITMAPINFOHEADER ih = {};
                ih.biSize = sizeof ih; ih.biWidth = LONG(w); ih.biHeight = -LONG(h); ih.biPlanes = 1; ih.biBitCount = 24;
                UINT stride = (w * 3 + 3) & ~3u;
                fh.bfType = 0x4D42; fh.bfOffBits = sizeof fh + sizeof ih; fh.bfSize = fh.bfOffBits + stride * h;
                fwrite(&fh, sizeof fh, 1, f);
                fwrite(&ih, sizeof ih, 1, f);
                std::vector<uint8_t> row(stride);
                for (UINT y = 0; y < h; y++)
                {
                    const uint8_t* s = mapped.bits + y * mapped.pitch;
                    for (UINT x = 0; x < w; x++)
                    {
                        // composite over a dark desktop-ish background
                        UINT a = s[x * 4 + 3], bg = 40;
                        for (int ch = 0; ch < 3; ch++) row[x * 3 + ch] = uint8_t(s[x * 4 + ch] + bg * (255 - a) / 255);
                    }
                    fwrite(row.data(), stride, 1, f);
                }
                fclose(f);
            }
            cpu->Unmap();
        }
    }
#endif
    return SUCCEEDED(hr);
}

void Renderer::BuildSprites(const Sim& sim, const Tracker& tr)
{
    dest.clear(); src.clear(); colors.clear(); transforms.clear();
    const float S = sim.S;

    // snow overhanging window corners, about to break off
    for (const SnowCap& c : sim.caps)
    {
        const Win* w = tr.Get(c.h);
        if (!w || !w->surface) continue;
        for (int side = 0; side < 2; side++)
        {
            float mass = side ? c.corniceR : c.corniceL;
            float s = sim.ScaleAt(side ? w->r : w->l);
            if (mass < 0.5f * s) continue;
            float dir = side ? 1.0f : -1.0f;
            float r = std::min(7.0f * s, 1.5f * s + mass * 0.45f);
            float x = (side ? w->r : w->l) + dir * r * 0.35f, y = w->t - r * 0.35f;
            if (tr.Covered(x, y, w->z)) continue;
            float d = r * 2.2f * k;
            dest.push_back(D2D1::RectF(-d / 2, -d / 2, d / 2, d / 2));
            src.push_back(D2D1::RectU(DotX, FlakeY, DotX + DotSize, FlakeY + DotSize));
            colors.push_back(D2D1::ColorF(0.99f, 0.98f, 0.96f, 0.97f));
            transforms.push_back(Place(1.3f, 0.8f, 0, x * k, y * k));
        }
    }

    // drops swelling at window corners, about to fall off like from a gutter
    for (const SnowCap& c : sim.caps)
    {
        const Win* w = tr.Get(c.h);
        if (!w || !w->surface) continue;
        for (int side = 0; side < 2; side++)
        {
            float bead = side ? c.beadR : c.beadL;
            if (bead < 0.08f * S) continue;
            float x = side ? w->r + 1.5f * S : w->l - 1.5f * S;
            if (tr.Covered(x, w->t + 2, w->z)) continue;
            float d = (2.5f + 4.5f * bead / (0.5f * S)) * S * k;
            dest.push_back(D2D1::RectF(-d / 2, -d / 2, d / 2, d / 2));
            src.push_back(D2D1::RectU(DotX, 0, DotX + DotSize, DotSize));
            colors.push_back(D2D1::ColorF(0.80f, 0.88f, 0.98f, 0.85f));
            transforms.push_back(Place(0.8f, 1.1f, 0, x * k, (w->t + d / (2 * k)) * k));
        }
    }

    auto leafPose = [&](const Particle& p, float& s, float& sx, float& sy, float& y, float& rot) {
        s = p.size * k; sx = 1; y = p.y; rot = p.rot;
        if (p.state == Falling)
        {
            // turning in 3D: squash along the leaf axis, never quite edge-on
            sy = p.flip;
            if (std::fabs(sy) < 0.2f) sy = sy < 0 ? -0.2f : 0.2f;
        }
        else
        {
            // settling down: it touched with its edge, now flops flat with a soft ease-out, the turn
            // overshooting a hair and swinging back
            float t = p.settle, e = 1 - (1 - t) * (1 - t) * (1 - t);
            float back = 1 + 2.2f * (t - 1) * (t - 1) * (t - 1) + 1.2f * (t - 1) * (t - 1);
            float d = std::fmod(p.restRot - p.startRot + 540.0f, 360.0f) - 180.0f;
            rot = p.startRot + d * back;
            sx = 1 + (0.45f - 1) * e;
            sy = p.startFlip + (1 - p.startFlip) * e;
            y -= p.size * (0.22f + (0.05f - 0.22f) * e);
        }
    };
    // smallest mip cell that is still at least as large as the leaf on screen
    auto level = [](float s) {
        UINT l = 0;
        while (l + 1 < Levels && float(Base >> (l + 1)) >= s) l++;
        return l;
    };

    if (shadows)
    {
        // soft drop shadows first, from a coarser mip so they come out blurred for free
        for (const Particle& p : sim.parts)
        {
            if (p.kind != KLeaf || p.hidden || p.alpha <= 0) continue;
            float s, sx, sy, y, rot;
            leafPose(p, s, sx, sy, y, rot);
            float lift = (p.state == Falling ? 9.0f : 2.5f) * S * k * p.depth;
            dest.push_back(D2D1::RectF(-s * 0.54f, -s * 0.54f, s * 0.54f, s * 0.54f));
            src.push_back(LeafCell(p.shape, p.color, std::min(Levels - 1, level(s) + 2)));
            colors.push_back(D2D1::ColorF(0, 0, 0, 0.22f * p.alpha));
            transforms.push_back(Place(sx, sy, rot, p.x * k + lift * 0.5f, y * k + lift));
        }
    }

    auto addLeaf = [&](const Particle& p) {
        float s, sx, sy, y, rot;
        leafPose(p, s, sx, sy, y, rot);
        dest.push_back(D2D1::RectF(-s / 2, -s / 2, s / 2, s / 2));
        src.push_back(LeafCell(p.shape, p.color, level(s)));
        colors.push_back(D2D1::ColorF(1, 1, 1, p.alpha));
        transforms.push_back(Place(sx, sy, rot, p.x * k, y * k));
    };
    // resting leaves bottom to top, so heaps overlap the right way round
    for (int i : sim.restOrder)
    {
        const Particle& p = sim.parts[i];
        if (!p.hidden && p.alpha > 0) addLeaf(p);
    }

    for (const Particle& p : sim.parts)
    {
        switch (p.kind)
        {
        case KLeaf:
        {
            if (p.state == Falling && !p.hidden && p.alpha > 0) addLeaf(p);
            break;
        }
        case KRain:
        {
            if (p.hidden) break;
            if (p.mode == 2)
            {
                // a drop running down a window's side: a short glossy bead elongated along the edge
                float len = 9.0f * S * k, w = 2.6f * S * k;
                dest.push_back(D2D1::RectF(-w / 2, -len / 2, w / 2, len / 2));
                src.push_back(D2D1::RectU(StreakX, 0, StreakX + StreakW, StreakH));
                colors.push_back(D2D1::ColorF(0.80f, 0.88f, 0.98f, 0.85f));
                transforms.push_back(Place(1, 1, 0, p.x * k, (p.y - 4.5f * S) * k));
                break;
            }
            float v = std::max(1.0f, std::sqrt(p.vx * p.vx + p.vy * p.vy));
            float len = std::max(4.0f * S * k, v * 0.022f * k), w = (1.2f + p.depth) * 1.6f * S * k;
            float cx = p.x - p.vx * 0.011f, cy = p.y - p.vy * 0.011f;   // centre: half a streak behind the head
            dest.push_back(D2D1::RectF(-w / 2, -len / 2, w / 2, len / 2));
            src.push_back(D2D1::RectU(StreakX, 0, StreakX + StreakW, StreakH));
            colors.push_back(D2D1::ColorF(0.70f, 0.78f, 0.90f, 0.30f + 0.40f * p.depth));
            transforms.push_back(Place(1, 1, -std::atan2(p.vx, p.vy) * 180.0f / Pi, cx * k, cy * k));
            break;
        }
        case KSnow:
        case KSplash:
        {
            bool snow = p.kind == KSnow;
            float d = p.size * 2.0f * k;
            dest.push_back(D2D1::RectF(-d / 2, -d / 2, d / 2, d / 2));
            src.push_back(snow ? D2D1::RectU(DotX, FlakeY, DotX + DotSize, FlakeY + DotSize) : D2D1::RectU(DotX, 0, DotX + DotSize, DotSize));
            float fade = snow ? 1.0f : 1 - p.age / p.life;
            colors.push_back(snow ? D2D1::ColorF(1.0f, 0.985f, 0.96f, 0.45f + 0.5f * p.depth)
                             : p.color == 1 ? D2D1::ColorF(0.40f, 0.28f, 0.16f, 0.95f * fade)
                             : p.color == 2 ? D2D1::ColorF(0.82f, 0.52f, 0.20f, 0.95f * fade)
                                            : D2D1::ColorF(0.75f, 0.84f, 0.95f, 0.8f * fade));
            transforms.push_back(Place(1, 1, 0, p.x * k, p.y * k));
            break;
        }
        case KRipple:
        {
            float t = p.age / p.life;
            float d = (3.0f + 22.0f * t) * S * p.depth * k;
            dest.push_back(D2D1::RectF(-d / 2, -d / 2, d / 2, d / 2));
            src.push_back(D2D1::RectU(DotX, RingY, DotX + DotSize, RingY + DotSize));
            colors.push_back(D2D1::ColorF(0.85f, 0.92f, 1.0f, 0.7f * (1 - t)));
            transforms.push_back(Place(1, 0.3f, 0, p.x * k, p.y * k));   // seen from the side: a flat ellipse
            break;
        }
        }
    }
}

void Renderer::DrawCaps(const Sim& sim, const Tracker& tr)
{
    const float B = float(SnowCap::Bucket);
    // an 8x8 solid white patch from the middle of the snowflake disc, and the soft dot for rims
    const D2D1_RECT_U solid = D2D1::RectU(DotX + 28, FlakeY + 28, DotX + 36, FlakeY + 36);
    const D2D1_RECT_U dot = D2D1::RectU(DotX, 0, DotX + DotSize, DotSize);
    ldest.clear(); lsrc.clear(); lcolors.clear(); ltransforms.clear();
    const D2D1_MATRIX_3X2_F identity = D2D1::Matrix3x2F::Identity();
    const float fade = 1.3f * k;   // width of the soft edge in pixels
    struct Pt { float x, t, b; };
    std::vector<Pt> run;

    // One layer of a surface, between bottom(j) and top(j) per bucket, drawn per visible run as a polyline
    // surface: between neighbouring points a solid body plus a skewed cap whose top follows the slope and
    // fades out over a pixel or so, then a soft rim line along the top.
    auto layer = [&](const SnowCap& c, const Win* w, auto bottom, auto top, D2D1_COLOR_F fill, D2D1_COLOR_F edge, float edgeWidth) {
        size_t n = c.height.size();
        float left = w ? w->l : 0;
        int z = w ? w->z : INT32_MAX;
        auto visible = [&](size_t j) {
            float b = bottom(j), t = top(j);
            return b - t > 0.3f && !tr.Covered(left + j * B + B / 2, (b + t) * 0.5f - 1, z, !w);
        };
        for (size_t i = 0; i < n;)
        {
            if (!visible(i)) { i++; continue; }
            size_t a = i;
            while (i < n && visible(i)) i++;
            // the run tapers to nothing at both ends, like the old outline did
            run.clear();
            run.push_back({ (left + a * B) * k, bottom(a) * k, bottom(a) * k });
            for (size_t j = a; j < i; j++) run.push_back({ (left + j * B + B / 2) * k, top(j) * k, bottom(j) * k });
            run.push_back({ (left + i * B) * k, bottom(i - 1) * k, bottom(i - 1) * k });

            for (size_t s = 0; s + 1 < run.size(); s++)
            {
                const Pt& p = run[s];
                const Pt& q = run[s + 1];
                float lower = std::max(p.t, q.t), floor = std::max(p.b, q.b);
                if (floor - lower - 1.0f > 0.01f)
                {
                    // starts where the cap ends, so translucent water isn't doubled up along the seam
                    ldest.push_back(D2D1::RectF(p.x, lower + 1.0f, q.x, floor));
                    lsrc.push_back(solid);
                    lcolors.push_back(fill);
                    ltransforms.push_back(identity);
                }
                // the cap: a parallelogram from the sloping top line down past the body's top edge
                float capH = std::fabs(p.t - q.t) + fade + 1.0f;
                UINT rows = UINT(std::max(float(SoftFade), std::min(float(SoftH), SoftFade * capH / fade)));
                D2D1_MATRIX_3X2_F m;
                m._11 = q.x - p.x; m._12 = q.t - p.t;
                m._21 = 0;         m._22 = capH;
                m._31 = p.x;       m._32 = p.t - fade;
                ldest.push_back(D2D1::RectF(0, 0, 1, 1));
                lsrc.push_back(D2D1::RectU(DotX, SoftY, DotX + SoftW, SoftY + rows));
                lcolors.push_back(fill);
                ltransforms.push_back(m);
                // rim along the top, a soft-ended line
                float dx = q.x - p.x, dy = q.t - p.t, len = std::sqrt(dx * dx + dy * dy);
                if (len > 0.01f && s > 0 && s + 2 < run.size())
                {
                    float ew = edgeWidth * k * 2.2f;
                    ldest.push_back(D2D1::RectF(-ew * 0.3f, -ew / 2, len + ew * 0.3f, ew / 2));
                    lsrc.push_back(dot);
                    lcolors.push_back(edge);
                    ltransforms.push_back(Place(1, 1, std::atan2(dy, dx) * 180 / Pi, p.x, p.t - fade * 0.5f));
                }
            }
        }
    };

    auto surface = [&](const SnowCap& c, const Win* w) {
        bool snow = c.AnySnow(), water = c.AnyWater(), mud = c.AnyMud();
        if (!snow && !water && !mud) return;
        auto base = [&](size_t j) { return w ? w->t : sim.groundY[std::min(j, sim.groundY.size() - 1)]; };
        auto mudTop = [&](size_t j) { return base(j) - c.mud[j]; };
        auto snowTop = [&](size_t j) { return mudTop(j) - c.height[j]; };
        auto waterTop = [&](size_t j) {
            float wv = c.wave[j] * std::min(1.0f, c.water[j] / (2.0f * sim.S));
            return snowTop(j) - c.water[j] - wv;
        };
        if (mud) layer(c, w, base, mudTop, D2D1::ColorF(0.36f, 0.25f, 0.15f, 0.94f), D2D1::ColorF(0.24f, 0.16f, 0.09f, 0.8f), 1.0f);
        if (snow) layer(c, w, mudTop, snowTop, D2D1::ColorF(0.99f, 0.98f, 0.96f, 0.97f), D2D1::ColorF(0.80f, 0.80f, 0.84f, 0.45f), 0.9f);
        if (water) layer(c, w, snowTop, waterTop, D2D1::ColorF(0.40f, 0.55f, 0.70f, 0.50f), D2D1::ColorF(0.92f, 0.96f, 1.0f, 0.65f), 1.0f);
    };

    for (const SnowCap& c : sim.caps)
    {
        const Win* w = tr.Get(c.h);
        if (w && w->surface) surface(c, w);
    }
    surface(sim.ground, nullptr);

    if (ldest.empty()) return;
    layerBatch->Clear();
    layerBatch->AddSprites(UINT32(ldest.size()), ldest.data(), lsrc.data(), lcolors.data(), ltransforms.data());
    dc->SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
    dc->DrawSpriteBatch(layerBatch.Get(), atlas.Get(), D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
}
bool Renderer::Render(const Sim& sim, const Tracker& tr)
{
    BuildSprites(sim, tr);

    dc->BeginDraw();
    dc->Clear(D2D1::ColorF(0, 0, 0, 0));

    dc->SetTransform(D2D1::Matrix3x2F::Identity());
    DrawCaps(sim, tr);
    DrawCritters(sim, tr);

    if (!dest.empty())
    {
        batch->Clear();
        batch->AddSprites(UINT32(dest.size()), dest.data(), src.data(), colors.data(), transforms.data());
        dc->SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // required by DrawSpriteBatch
        dc->DrawSpriteBatch(batch.Get(), atlas.Get(), D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
    }

    if (sim.flash > 0)
    {
        flashBrush->SetOpacity(sim.flash * 0.28f);
        flashBrush->SetColor(D2D1::ColorF(0.85f, 0.9f, 1.0f, 1.0f));
        dc->FillRectangle(D2D1::RectF(0, 0, sim.W * k, sim.H * k), flashBrush.Get());
    }

    if (FAILED(dc->EndDraw())) return false;
    // synced to the display: each frame stays up for the same number of refreshes
    HRESULT hr = swap->Present(syncInterval, 0);
    return hr != DXGI_ERROR_DEVICE_REMOVED && hr != DXGI_ERROR_DEVICE_RESET;
}

bool Renderer::Clear()
{
    dc->BeginDraw();
    dc->Clear(D2D1::ColorF(0, 0, 0, 0));
    if (FAILED(dc->EndDraw())) return false;
    return SUCCEEDED(swap->Present(0, 0));
}
