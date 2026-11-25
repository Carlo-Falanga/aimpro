// La scena di aimpro: la stanza a griglia, le sfere (calcolate per pixel, senza mesh) e i rettangoli del mirino e
// del tempo. Niente texture né buffer di profondità: la stanza è un parallelepipedo visto da dentro, ogni pixel ne
// vede una faccia sola, e le sfere si disegnano sopra (si toccano di rado: l'ordine delle istanze basta).

// Quello che cambia a ogni frame arriva come costanti immediate (root constants di DirectX 12, 128 byte): stanno
// nella lista dei comandi, senza buffer da copiare.
struct Immediati {
    vp: mat4x4<f32>,
    // x tempo di gioco in secondi (il moto delle sfere), y indice della sfera guardata (-1 nessuna),
    // z opacità del lampo della sfera colpita, w indice dell'istanza del lampo (-1 nessuno)
    tempo: vec4<f32>,
    destra: vec4<f32>,
    su: vec4<f32>,
    // larghezza, altezza in pixel, frazione di tempo rimasta, scala dell'interfaccia
    schermo: vec4<f32>,
};

var<immediate> f: Immediati;

// Una sfera: il suo tratto di moto. moto = imbardata e beccheggio di partenza, velocità (radianti al secondo);
// altro = istante di partenza (secondi di gioco), raggio, distanza dall'occhio, istante in cui compare.
struct Sfera {
    moto: vec4<f32>,
    altro: vec4<f32>,
};

// Quello che nel giro cambia di rado (l'occhio, i colori dei bersagli, il tema della stanza, la zona dei rimbalzi e
// i tratti delle sfere) sta in un buffer, riscritto solo quando cambia: a una sfera colpita o a un cambio di
// direzione, non a ogni frame.
struct Uniformi {
    occhio: vec4<f32>,
    bersaglio_normale: vec4<f32>,
    bersaglio_guardato: vec4<f32>,
    bersaglio_colpito: vec4<f32>,
    pavimento: vec4<f32>,
    soffitto: vec4<f32>,
    pareti: vec4<f32>,
    linee: vec4<f32>,
    // imbardata minima, massima, beccheggio minimo, massimo dei centri
    zona: vec4<f32>,
    // le sfere in campo e, subito dopo l'ultima, quella del lampo (ferma dove è stata colpita)
    sfere: array<Sfera, 9>,
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

// ---------------------------------------------------------------- sfere

// x riportato in [lo, hi] come una palla che rimbalza ai bordi: lo stesso conto di `giro::rifletti`.
fn rifletti(x: f32, lo: f32, hi: f32) -> f32 {
    let w = hi - lo;
    if (w <= 0.0) {
        return lo;
    }
    let d = x - lo;
    let p = d - 2.0 * w * floor(d / (2.0 * w));
    return select(lo + 2.0 * w - p, lo + p, p <= w);
}

// Il centro della sfera al tempo di gioco di questo frame, come in `Giro::centro_a`.
fn centro(s: Sfera) -> vec3<f32> {
    let dt = f.tempo.x - s.altro.x;
    let imb = rifletti(s.moto.x + s.moto.z * dt, u.zona.x, u.zona.y);
    let bec = rifletti(s.moto.y + s.moto.w * dt, u.zona.z, u.zona.w);
    let cp = cos(bec);
    return u.occhio.xyz + vec3<f32>(sin(imb) * cp, sin(bec), -cos(imb) * cp) * s.altro.z;
}

struct UscitaSfera {
    @builtin(position) pos: vec4<f32>,
    @location(0) mondo: vec3<f32>,
    // xyz centro, w raggio: uguali per tutto il quadrato
    @location(1) @interpolate(flat) sfera: vec4<f32>,
    @location(2) @interpolate(flat) indice: u32,
};

@vertex
fn vs_sfera(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> UscitaSfera {
    // Un quadrato di due triangoli rivolto alla camera, un po' più grande della sfera; un'istanza per sfera.
    var angoli = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let s = u.sfere[i];
    let c = centro(s);
    let r = s.altro.y;
    let a = angoli[v];
    let p = c + (f.destra.xyz * a.x + f.su.xyz * a.y) * r * 1.6;
    var o: UscitaSfera;
    o.pos = f.vp * vec4<f32>(p, 1.0);
    if (f.tempo.x < s.altro.w) {
        // Non ancora comparsa (scenari di reazione): il quadrato va fuori dal volume visibile e non si disegna.
        o.pos = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    }
    o.mondo = p;
    o.sfera = vec4<f32>(c, r);
    o.indice = i;
    return o;
}

// Il colore della sfera nel punto con normale n, visto in direzione rd.
fn colore_sfera(n: vec3<f32>, rd: vec3<f32>, indice: u32) -> vec3<f32> {
    let luce = normalize(vec3<f32>(0.35, 0.8, 0.45));
    let diffusa = max(dot(n, luce), 0.0);
    let bordo = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    return vec3<f32>(1.0, 0.69, 0.13) * (0.38 + 0.62 * diffusa) + vec3<f32>(1.0, 0.9, 0.6) * bordo * 0.35;
}

@fragment
fn fs_sfera(i: UscitaSfera) -> @location(0) vec4<f32> {
    let c = i.sfera.xyz;
    let r = i.sfera.w;
    let rd = normalize(i.mondo - u.occhio.xyz);
    let oc = c - u.occhio.xyz;
    let b = dot(oc, rd);
    let d2 = dot(oc, oc) - b * b;
    let dist = sqrt(max(d2, 0.0)) - r;
    // Bordo morbido largo un pixel.
    // L'istanza del lampo sfuma; le altre sono piene.
    let lampo = f32(i.indice) == f.tempo.w;
    let alfa = clamp(0.5 - dist / max(fwidth(dist), 1e-6), 0.0, 1.0) * select(1.0, f.tempo.z, lampo);
    if (alfa <= 0.0) {
        discard;
    }
    let h = sqrt(max(r * r - d2, 0.0));
    let punto = u.occhio.xyz + rd * (b - h);
    let n = normalize(punto - c);
    // Lo stato per il colore: 2 la sfera del lampo, 1 quella guardata, 0 le altre.
    var stato = 0.0;
    if (lampo) {
        stato = 2.0;
    } else if (f32(i.indice) == f.tempo.y) {
        stato = 1.0;
    }
    let luce = normalize(vec3<f32>(0.35, 0.8, 0.45));
    let diffusa = max(dot(n, luce), 0.0);
    let bordo = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    let colore = colore_bersaglio(stato);
    // Il riflesso sul bordo: lo stesso colore schiarito (con l'arancione di base è quello di prima).
    let col = colore * (0.38 + 0.62 * diffusa) + mix(colore, vec3<f32>(1.0), 0.6) * bordo * 0.35;
    return vec4<f32>(col * alfa, alfa);
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
