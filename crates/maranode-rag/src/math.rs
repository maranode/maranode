//! embedding vector math and binary layout on disk

pub fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < f32::EPSILON || nb < f32::EPSILON {
        return 0.0;
    }
    dot(a, b) / (na * nb)
}

pub fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

pub fn blob_to_vec(b: &[u8]) -> anyhow::Result<Vec<f32>> {
    if b.len() % 4 != 0 {
        anyhow::bail!("embedding blob length {} is not a multiple of 4", b.len());
    }
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let v = vec![0.1f32, -2.5, 3.0, 0.0];
        let blob = vec_to_blob(&v);
        let back = blob_to_vec(&blob).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn blob_rejects_bad_length() {
        assert!(blob_to_vec(&[0u8, 1, 2]).is_err());
    }

    #[test]
    fn normalised_dot_is_cosine() {
        let mut a = vec![3.0f32, 4.0];
        let mut b = vec![4.0f32, 3.0];
        let raw_cos = cosine(&a, &b);
        normalize(&mut a);
        normalize(&mut b);
        let dotted = dot(&a, &b);
        assert!((raw_cos - dotted).abs() < 1e-6);
    }

    #[test]
    fn identical_vectors_have_cosine_one() {
        let a = vec![1.0f32, 2.0, 3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }
}
