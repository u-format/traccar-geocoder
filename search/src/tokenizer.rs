pub fn normalize_turkish(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ş' | 'Ş' => 's',
            'ğ' | 'Ğ' => 'g',
            'ü' | 'Ü' => 'u',
            'ö' | 'Ö' => 'o',
            'ı'       => 'i',
            'İ'       => 'i',
            'ç' | 'Ç' => 'c',
            other => other.to_lowercase().next().unwrap_or(other),
        })
        .collect()
}
