// La scena di aimpro: la stanza a griglia, la sfera (calcolata per pixel, senza mesh) e i rettangoli del mirino e
// del tempo. Niente texture né buffer di profondità: la stanza è un parallelepipedo visto da dentro, ogni pixel ne
// vede una faccia sola, e la sfera si disegna sopra.

// Quello che cambia a ogni frame arriva come costanti immediate (root constants di DirectX 12, 128 byte): stanno
// nella lista dei comandi, senza buffer da copiare.
struct Immediati {
    vp: mat4x4<f32>,
    // xyz centro della sfera, w raggio (0 = nessuna sfera)
    sfera: vec4<f32>,
    // w: lo stato della sfera per il suo colore (0 normale, 1 guardata, 2 colpita)
    destra: vec4<f32>,
    // w: l'opacità della sfera (1, meno nel lampo del colpo)
    su: vec4<f32>,
    // larghezza, altezza in pixel, frazione di tempo rimasta, scala dell'interfaccia
    schermo: vec4<f32>,
};

var<immediate> f: Immediati;

// Quello che non cambia nel giro (l'occhio, i colori dei bersagli, il tema della stanza) sta in un buffer,
// riscritto solo quando cambia.
struct Uniformi {
    occhio: vec4<f32>,
    bersaglio_normale: vec4<f32>,
    bersaglio_guardato: vec4<f32>,
    bersaglio_colpito: vec4<f32>,
    pavimento: vec4<f32>,
    soffitto: vec4<f32>,
    pareti: vec4<f32>,
    linee: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniformi;

// Il colore di un bersaglio dal suo stato: 0 normale, 1 guardato (il mirino ci sta sopra), 2 colpito.
fn colore_bersaglio(stato: f32) -> vec3<f32> {
    if (stato > 1.5) {
        return u.bersaglio_colpito.rgb;
    }
    if (stato > 0.5) {
        return u.bersaglio_guardato.rgb;
    }
    return u.bersaglio_normale.rgb;
}

// ---------------------------------------------------------------- stanza

struct UscitaStanza {
    @builtin(position) pos: vec4<f32>,
    @location(0) mondo: vec3<f32>,
    @location(1) normale: vec3<f32>,
};

@vertex
fn vs_stanza(@location(0) pos: vec3<f32>, @location(1) normale: vec3<f32>) -> UscitaStanza {
    var o: UscitaStanza;
    o.pos = f.vp * vec4<f32>(pos, 1.0);
    o.mondo = pos;
    o.normale = normale;
    return o;
}

fn linee(uv: vec2<f32>, passo: f32) -> f32 {
    let q = uv / passo;
    let d = abs(fract(q - 0.5) - 0.5) / fwidth(q);
    return 1.0 - min(min(d.x, d.y), 1.0);
}

@fragment
fn fs_stanza(i: UscitaStanza) -> @location(0) vec4<f32> {
    let n = abs(i.normale);
    var uv: vec2<f32>;
    var base: vec3<f32>;
    // La normale del pavimento guarda in alto (y > 0): pavimento; in basso: soffitto.
    if (n.y > 0.5) {
        uv = i.mondo.xz;
        base = select(u.soffitto.rgb, u.pavimento.rgb, i.normale.y > 0.0);
    } else if (n.x > 0.5) {
        uv = i.mondo.zy;
        base = u.pareti.rgb;
    } else {
        uv = i.mondo.xy;
        base = u.pareti.rgb;
    }
    let l = max(linee(uv, 1.0) * 0.35, linee(uv, 4.0));
    var c = mix(base, u.linee.rgb, l * 0.8);
    let distanza = length(i.mondo - u.occhio.xyz);
    c = c * mix(1.0, 0.72, clamp(distanza / 30.0, 0.0, 1.0));
    return vec4<f32>(c, 1.0);
}

// ---------------------------------------------------------------- sfera

struct UscitaSfera {
    @builtin(position) pos: vec4<f32>,
    @location(0) mondo: vec3<f32>,
};

@vertex
fn vs_sfera(@builtin(vertex_index) v: u32) -> UscitaSfera {
    // Un quadrato di due triangoli rivolto alla camera, un po' più grande della sfera.
    var angoli = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let a = angoli[v];
    let p = f.sfera.xyz + (f.destra.xyz * a.x + f.su.xyz * a.y) * f.sfera.w * 1.6;
    var o: UscitaSfera;
    o.pos = f.vp * vec4<f32>(p, 1.0);
    o.mondo = p;
    return o;
}

@fragment
fn fs_sfera(i: UscitaSfera) -> @location(0) vec4<f32> {
    let r = f.sfera.w;
    let rd = normalize(i.mondo - u.occhio.xyz);
    let oc = f.sfera.xyz - u.occhio.xyz;
    let b = dot(oc, rd);
    let d2 = dot(oc, oc) - b * b;
    let dist = sqrt(max(d2, 0.0)) - r;
    // Bordo morbido largo un pixel.
    let alfa = clamp(0.5 - dist / max(fwidth(dist), 1e-6), 0.0, 1.0) * f.su.w;
    if (alfa <= 0.0) {
        discard;
    }
    let h = sqrt(max(r * r - d2, 0.0));
    let punto = u.occhio.xyz + rd * (b - h);
    let n = normalize(punto - f.sfera.xyz);
    let luce = normalize(vec3<f32>(0.35, 0.8, 0.45));
    let diffusa = max(dot(n, luce), 0.0);
    let bordo = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    let colore = colore_bersaglio(f.destra.w);
    // Il riflesso sul bordo: lo stesso colore schiarito (con l'arancione di base è quello di prima).
    let c = colore * (0.38 + 0.62 * diffusa) + mix(colore, vec3<f32>(1.0), 0.6) * bordo * 0.35;
    return vec4<f32>(c * alfa, alfa);
}

// ---------------------------------------------------------------- rettangoli (mirino e tempo)

struct UscitaRett {
    @builtin(position) pos: vec4<f32>,
    @location(0) colore: vec4<f32>,
    @location(1) locale: vec2<f32>,
    // raggio interno, raggio esterno, tipo
    @location(2) anello: vec3<f32>,
};

@vertex
fn vs_rett(
    @builtin(vertex_index) v: u32,
    @location(0) rett: vec4<f32>,
    @location(1) colore: vec4<f32>,
    @location(2) tipo: f32,
    @location(3) anello: vec2<f32>,
) -> UscitaRett {
    var angoli = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    var larghezza = rett.z;
    if (tipo > 0.5 && tipo < 1.5) {
        // La barra del tempo: la larghezza segue la frazione rimasta, senza riscrivere il buffer.
        larghezza = rett.z * f.schermo.z;
    }
    let p = rett.xy + angoli[v] * vec2<f32>(larghezza, rett.w);
    let c = p / f.schermo.xy * 2.0 - 1.0;
    var o: UscitaRett;
    o.pos = vec4<f32>(c.x, -c.y, 0.0, 1.0);
    o.colore = vec4<f32>(colore.rgb * colore.a, colore.a);
    // Per l'anello: la distanza in pixel dal centro del rettangolo e i due raggi.
    o.locale = (angoli[v] - 0.5) * vec2<f32>(larghezza, rett.w);
    o.anello = vec3<f32>(anello, tipo);
    return o;
}

@fragment
fn fs_rett(i: UscitaRett) -> @location(0) vec4<f32> {
    if (i.anello.z > 1.5) {
        // Anello del mirino a cerchio, con il bordo morbido largo un pixel.
        let d = length(i.locale);
        let a = clamp(d - i.anello.x + 0.5, 0.0, 1.0) * clamp(i.anello.y - d + 0.5, 0.0, 1.0);
        if (a <= 0.0) {
            discard;
        }
        return i.colore * a;
    }
    return i.colore;
}
