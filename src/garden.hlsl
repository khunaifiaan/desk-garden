cbuffer Frame : register(b0) {
    float2 screen; float scale; float time;
    float2 offset; float swayFern; float swayCanopy;
    float3 gradeMul; float night;
    float4 gradeScreen;                 // rgb, alpha
    float desat; float contrast; float bright; float layerDesat;
    float3 weatherMul; float layerBright;
    float basinLevel; float spoutPhase; float splash; float rippleCount;
    float4 fernRect;                    // overlay textures are cropped: x, y, w, h in design px
    float4 canopyRect;
    float4 ripples[32];                 // x, y, age, unused
};
Texture2D baseTex   : register(t0);
Texture2D rakeMask  : register(t1);     // bed * (1 - objects)
Texture2D fernTex   : register(t2);
Texture2D canopyTex : register(t3);
SamplerState lin : register(s0);
SamplerState border : register(s1);  // transparent outside cropped overlays

static const float2 DESIGN = float2(1920, 1080);
static const float3 LUMA = float3(0.2126, 0.7152, 0.0722);

float3 saturateBy(float3 c, float s) { return lerp(dot(c, LUMA).xxx, c, s); }

// Grade applied to everything drawn "before the grade" in the mockup:
// time-of-day multiply + screen tint, soft vignette, then the weather multiply.
float3 postGrade(float3 c, float2 d) {
    c *= gradeMul;
    float3 scr = 1 - (1 - c) * (1 - gradeScreen.rgb);
    c = lerp(c, scr, gradeScreen.a);
    float r = length(d - DESIGN * 0.5);
    float v = saturate((r - 378.0) / (1248.0 - 378.0)) * 0.28;
    c *= 1 - v;
    c *= weatherMul;
    return c;
}

float4 vs_full(uint id : SV_VertexID) : SV_Position {
    float2 p = float2((id << 1) & 2, id & 2);
    return float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
}

float2 rot(float2 p, float a) { float s = sin(a), c = cos(a); return float2(c * p.x - s * p.y, s * p.x + c * p.y); }

// Ellipse ring coverage: distance of p from ellipse (radius r, y squashed by sy) vs stroke width w.
float ring(float2 p, float2 c, float r, float sy, float w, float aa) {
    float2 q = (p - c) / float2(1, sy);
    float e = abs(length(q) - r);
    return 1 - smoothstep(w * 0.5 - aa * 0.5, w * 0.5 + aa * 0.5, e);
}
float disc(float2 p, float2 c, float r, float sy, float aa) {
    float2 q = (p - c) / float2(1, sy);
    return 1 - smoothstep(r - aa * 0.5, r + aa * 0.5, length(q));
}

float4 overlay(Texture2D t, float4 rc, float2 d, float2 pivot, float ang) {
    float2 q = rot(d - pivot, -ang) + pivot;
    return t.SampleLevel(border, (q - rc.xy) / rc.zw, 0);
}
float shadowA(Texture2D t, float4 rc, float2 d, float2 pivot, float ang, float2 off, float lod) {
    float2 q = rot(d - off - pivot, -ang) + pivot;
    return t.SampleLevel(border, (q - rc.xy) / rc.zw, lod).a;
}

float4 ps_scene(float4 pos : SV_Position) : SV_Target {
    float2 d = (pos.xy - offset) / scale;
    float2 uv = d / DESIGN;
    float aa = 1.0 / scale;                    // one screen pixel in design units
    float3 c = baseTex.SampleLevel(lin, uv, 0).rgb;

    // weather filter on the ground (CSS saturate/contrast/brightness order)
    c = saturateBy(c, 1 - desat);
    c = saturate((c - 0.5) * contrast + 0.5);
    c *= bright;

    // --- stone basin (tsukubai): wall, water level, spout drip rings
    if (basinLevel > 0) {
        float2 B = float2(245, 600); float R = 40.5;
        float wallA = disc(d, B, R, 0.95, aa);
        if (wallA > 0) {
            float tw = saturate((length(d - (B - 9)) - 3) / (R - 3));
            float3 wall = lerp(float3(0x5b, 0x5e, 0x55), float3(0x2f, 0x31, 0x2b), tw) / 255;
            c = lerp(c, wall, wallA);
            float wr = R * basinLevel;
            float waterA = disc(d, B, wr, 0.95, aa);
            float tq = saturate((length(d - (B - 7.5)) - 1.5) / max(wr - 1.5, 1));
            float3 water = lerp(float3(0x5a, 0x9a, 0x8f), float3(0x16, 0x3a, 0x39), tq) / 255;
            c = lerp(c, water, waterA);
            float2 S = float2(239, 595.5);
            [unroll] for (int k = 0; k < 3; k++) {
                float u = frac(spoutPhase + k / 3.0);
                float r = 3 + u * R * basinLevel * 0.9;
                float a = ring(d, S, r, 0.95, 1.95, aa) * (1 - u) * 0.8 * 0.6;
                c = lerp(c, float3(205, 236, 230) / 255, a * waterA);
            }
            float2 drop = S + float2(1.5 * sin(time * 23), 0);
            c = lerp(c, float3(230, 248, 244) / 255, disc(d, drop, 3 + splash * 3, 1, aa) * 0.81);
        }
    }

    // --- cursor rake ripples, masked to the gravel bed (not the stones)
    if (rippleCount > 0) {
        float m = rakeMask.SampleLevel(lin, uv, 0).r;
        if (m > 0.003) {
            for (int i = 0; i < (int)rippleCount; i++) {
                float4 r = ripples[i];
                float rr = 6 + r.z * 105;
                if (abs(length(d - r.xy) - rr) > 12) continue;
                float a = saturate(1 - r.z / 1.6) * 0.55 * m;
                c = lerp(c, float3(60, 66, 56) / 255, ring(d, r.xy, rr, 0.95, 3, aa) * a);
                c = lerp(c, float3(255, 255, 248) / 255, ring(d, r.xy + float2(-1.5, -2.25), rr, 0.95, 3, aa) * a);
            }
        }
    }

    // --- fern and canopy: blurred drop shadow, then the swaying layer itself
    float sf = shadowA(fernTex, fernRect, d, float2(1800, 1080), swayFern, float2(13.5, 19.5), 4.0);
    c *= 1 - 0.45 * sf;
    float4 f = overlay(fernTex, fernRect, d, float2(1800, 1080), swayFern);
    float3 fc = saturateBy(f.rgb, 1 - layerDesat) * layerBright;
    c = lerp(c, fc, f.a);

    float sc = shadowA(canopyTex, canopyRect, d, float2(960, -195), swayCanopy, float2(33, 48), 5.5);
    c *= 1 - 0.55 * sc;
    float4 k = overlay(canopyTex, canopyRect, d, float2(960, -195), swayCanopy);
    float3 kc = saturateBy(k.rgb, 1 - layerDesat) * layerBright;
    c = lerp(c, kc, k.a);

    return float4(postGrade(c, d), 1);
}

// ---------------- particles ----------------
struct PIn {
    float2 pos : POS;        // design px (centre)
    float2 ext : EXT;        // quad half extents (design px)
    float4 col : COL;        // straight rgba
    float4 prm : PRM;        // kind, quad rotation, stroke width / leaf radius, graded
    float4 shp : SHP;        // ring: rx, ry ; leaf: rot, squash
};
struct PV {
    float4 pos : SV_Position;
    float2 local : LOC;
    float2 design : DES;
    nointerpolation float4 col : COL;
    nointerpolation float4 prm : PRM;
    nointerpolation float4 shp : SHP;
    nointerpolation float2 ext : EXT;
};
PV vs_particle(PIn i, uint vid : SV_VertexID) {
    static const float2 C[6] = { float2(-1,-1), float2(1,-1), float2(-1,1), float2(-1,1), float2(1,-1), float2(1,1) };
    float2 l = C[vid] * i.ext;
    float2 d = i.pos + rot(l, i.prm.y);
    float2 s = d * scale + offset;
    PV o;
    o.pos = float4(s / screen * float2(2, -2) + float2(-1, 1), 0, 1);
    o.local = l; o.design = d; o.col = i.col; o.prm = i.prm; o.shp = i.shp; o.ext = i.ext;
    return o;
}
float4 ps_particle(PV v) : SV_Target {
    float aa = 1.0 / scale;
    float kind = v.prm.x;
    float a = 0;
    if (kind < 0.5) {            // ring
        float2 q = v.local / float2(v.shp.x, v.shp.y);
        float e = abs(length(q) - 1) * v.shp.x;
        a = 1 - smoothstep(v.prm.z * 0.5 - aa * 0.5, v.prm.z * 0.5 + aa * 0.5, e);
    } else if (kind < 1.5) {     // disc
        float2 q = v.local / float2(v.shp.x, v.shp.y);
        a = 1 - smoothstep(v.shp.x - aa * 0.5, v.shp.x + aa * 0.5, length(q) * v.shp.x);
    } else if (kind < 2.5) {     // streak: rectangle along the local x axis
        float2 e = v.ext - abs(v.local);
        a = saturate(e.x / aa) * saturate(e.y / aa);
    } else {                     // maple leaf: 10-point star, alternating radius 1 / 0.45
        float2 q = float2(v.local.x, v.local.y / v.shp.y);
        float R = v.prm.z;
        float ang = atan2(q.y, q.x) - v.shp.x;
        float seg = 6.2831853 / 10;
        float k = floor(ang / seg);
        float a0 = v.shp.x + k * seg, a1 = a0 + seg;
        float r0 = (fmod(abs(k), 2) < 0.5) ? R : R * 0.45;
        float r1 = (fmod(abs(k), 2) < 0.5) ? R * 0.45 : R;
        float2 p0 = float2(cos(a0), sin(a0)) * r0, p1 = float2(cos(a1), sin(a1)) * r1;
        float2 e = p1 - p0;
        float side = (e.x * (q.y - p0.y) - e.y * (q.x - p0.x)) / length(e);
        a = saturate(side / aa + 0.5);
    }
    float4 c = v.col;
    c.a *= a;
    if (c.a <= 0.002) discard;
    if (v.prm.w > 0.5) c.rgb = postGrade(c.rgb, v.design);
    return c;
}
