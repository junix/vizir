use vizir_core::map_linear;

#[test]
fn tiny_nonconstant_linear_domains_map_endpoints_and_interior() {
    let smallest = f64::from_bits(1);
    for domain in [
        [1e-120, 3e-120],
        [-3e-120, -1e-120],
        [-1e-120, 1e-120],
        [smallest, 3.0 * smallest],
        [-3.0 * smallest, -smallest],
        [-smallest, smallest],
        [0.0, smallest],
    ] {
        for domain in [domain, [domain[1], domain[0]]] {
            for range in [[20.0, 620.0], [340.0, 40.0]] {
                assert_eq!(map_linear(domain[0], domain, range), range[0], "{domain:?}");
                assert_eq!(map_linear(domain[1], domain, range), range[1], "{domain:?}");
                let middle = domain[0] + (domain[1] - domain[0]) / 2.0;
                let position = map_linear(middle, domain, range);
                assert!(position.is_finite());
                assert!((range[0].min(range[1])..=range[0].max(range[1])).contains(&position));
                if middle != domain[0] && middle != domain[1] {
                    assert!((position - (range[0] + range[1]) / 2.0).abs() < 1e-10);
                }
            }
        }
    }
}

#[test]
fn constant_and_ordinary_linear_mapping_keep_legacy_behavior() {
    for domain in [[0.0, 0.0], [5.0, 5.0], [-0.0, 0.0]] {
        assert_eq!(map_linear(domain[0], domain, [20.0, 620.0]), 320.0);
    }
    assert_eq!(map_linear(2.0, [1.0, 3.0], [20.0, 620.0]), 320.0);
    assert_eq!(map_linear(4.0, [1.0, 3.0], [20.0, 620.0]), 920.0);
}
