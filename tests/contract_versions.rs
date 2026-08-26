#[test]
fn external_contract_versions_are_explicit() {
    assert_eq!(positorium::TRAQULA_VERSION, 1);

    #[cfg(feature = "server")]
    {
        assert_eq!(positorium::server::HTTP_API_VERSION, "v1");
        assert_eq!(positorium::server::SSE_SCHEMA_VERSION, 1);
    }

    #[cfg(feature = "wasm")]
    assert_eq!(positorium::wasm::WASM_INTERFACE_VERSION, "1");

    #[cfg(feature = "persistence")]
    {
        assert_eq!(positorium::maintenance::LOGICAL_EXPORT_VERSION, 1);
        assert_eq!(positorium::maintenance::IDENTITY_REMAP_VERSION, 1);
    }
}
