//! Campioni delle misure in anelli di dimensione fissa: aggiungere un campione nel ciclo dei frame non alloca.
//! Il riassunto (che ordina una copia) si fa fuori dal ciclo, quando il pannello o il banco lo chiedono.

/// Un array di zeri direttamente sullo heap: `Box::new([0.0; N])` in debug lo costruisce prima sullo stack, e con
/// 512 KB per anello lo stack del thread principale (1 MB su Windows) finisce.
pub fn zeri<const N: usize>() -> Box<[f64; N]> {
    vec![0.0; N].into_boxed_slice().try_into().expect("lunghezza N")
}

pub struct Anello<const N: usize> {
    valori: Box<[f64; N]>,
    prossimo: usize,
    /// Campioni aggiunti dall'ultimo svuotamento (anche quelli già usciti dall'anello).
    pub totale: u64,
}

impl<const N: usize> Default for Anello<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Anello<N> {
    pub fn new() -> Self {
        Anello { valori: zeri(), prossimo: 0, totale: 0 }
    }

    #[inline]
    pub fn aggiungi(&mut self, v: f64) {
        self.valori[self.prossimo] = v;
        self.prossimo = (self.prossimo + 1) % N;
        self.totale += 1;
    }

    pub fn svuota(&mut self) {
        self.prossimo = 0;
        self.totale = 0;
    }

    /// Copia in `dest` gli ultimi `k` campioni (al più quelli presenti e al più `dest.len()`), in un ordine
    /// qualsiasi. Dice quanti ne ha copiati. Non alloca.
    pub fn ultimi(&self, k: usize, dest: &mut [f64]) -> usize {
        let n = k.min((self.totale as usize).min(N)).min(dest.len());
        // I più recenti stanno subito prima di `prossimo`, girando intorno alla fine dell'anello.
        let fine = self.prossimo;
        let dalla_fine = n.min(fine);
        dest[..dalla_fine].copy_from_slice(&self.valori[fine - dalla_fine..fine]);
        let resto = n - dalla_fine;
        dest[dalla_fine..n].copy_from_slice(&self.valori[N - resto..N]);
        n
    }

    /// I campioni presenti, in un ordine qualsiasi.
    pub fn campioni(&self) -> &[f64] {
        let n = (self.totale as usize).min(N);
        &self.valori[..n]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Riassunto {
    pub n: usize,
    pub mediana: f64,
    pub p99: f64,
    pub p999: f64,
    pub media: f64,
    pub deviazione: f64,
    pub massimo: f64,
}

fn percentile(ordinati: &[f64], p: f64) -> f64 {
    let pos = p * (ordinati.len() - 1) as f64;
    let i = pos.floor() as usize;
    let resto = pos - i as f64;
    match ordinati.get(i + 1) {
        Some(b) => ordinati[i] + resto * (b - ordinati[i]),
        None => ordinati[i],
    }
}

/// Mediana, 99° e 99,9° percentile (interpolati), media, deviazione standard e massimo. Ordina `scratch`, che
/// deve essere lungo almeno quanto `valori`.
pub fn riassumi(valori: &[f64], scratch: &mut [f64]) -> Riassunto {
    let v = &mut scratch[..valori.len()];
    v.copy_from_slice(valori);
    riassumi_sul_posto(v)
}

/// Come `riassumi`, ma ordina direttamente `v`.
pub fn riassumi_sul_posto(v: &mut [f64]) -> Riassunto {
    let n = v.len();
    if n == 0 {
        return Riassunto::default();
    }
    v.sort_unstable_by(f64::total_cmp);
    let media = v.iter().sum::<f64>() / n as f64;
    let varianza = v.iter().map(|x| (x - media) * (x - media)).sum::<f64>() / n as f64;
    Riassunto {
        n,
        mediana: percentile(v, 0.5),
        p99: percentile(v, 0.99),
        p999: percentile(v, 0.999),
        media,
        deviazione: varianza.sqrt(),
        massimo: v[n - 1],
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn l_anello_tiene_gli_ultimi_n() {
        let mut a = Anello::<3>::new();
        for v in 1..=5 {
            a.aggiungi(f64::from(v));
        }
        let mut c = a.campioni().to_vec();
        c.sort_by(f64::total_cmp);
        assert_eq!(c, vec![3.0, 4.0, 5.0]);
        assert_eq!(a.totale, 5);
        a.svuota();
        assert!(a.campioni().is_empty());
    }

    #[test]
    fn riassunto_da_1_a_100() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let mut s = vec![0.0; 100];
        let r = riassumi(&v, &mut s);
        assert_eq!(r.n, 100);
        assert_eq!(r.mediana, 50.5);
        assert!((r.p99 - 99.01).abs() < 1e-9);
        assert_eq!(r.media, 50.5);
        assert!((r.deviazione - 28.866).abs() < 1e-3);
        assert_eq!(r.massimo, 100.0);
    }

    #[test]
    fn p999_e_ultimi_campioni() {
        let v: Vec<f64> = (1..=1000).map(f64::from).collect();
        let mut s = vec![0.0; 1000];
        let r = riassumi(&v, &mut s);
        assert!((r.p999 - 999.001).abs() < 1e-9);
        // Gli ultimi 4 campioni di un anello che ne ha visti 10 (e ne tiene 8).
        let mut a = Anello::<8>::new();
        for x in 1..=10 {
            a.aggiungi(f64::from(x));
        }
        let mut d = [0.0; 4];
        let n = a.ultimi(4, &mut d);
        let mut u = d[..n].to_vec();
        u.sort_by(f64::total_cmp);
        assert_eq!(u, vec![7.0, 8.0, 9.0, 10.0]);
        let mut d = [0.0; 20];
        assert_eq!(a.ultimi(20, &mut d), 8);
        assert_eq!(r.massimo, 1000.0);
    }

    #[test]
    fn un_valore_solo_e_nessuno() {
        let mut s = vec![0.0; 4];
        let r = riassumi(&[7.0], &mut s);
        assert_eq!((r.mediana, r.p99, r.deviazione), (7.0, 7.0, 0.0));
        assert_eq!(riassumi(&[], &mut s).n, 0);
    }
}
