// Dev-only mock project (MVP).
//
// The `Project` DTO is part of the frozen contract (`model::Project`),
// so the mock kernel receives exactly the shape the Rust side would
// parse. This file only exists because the *host* has to hand the
// kernel a project to load — the kernel never invents one.
//
// Kept in `front/dev/` because it is demo data: the production path
// (M-06 ELF / raw-HEX loader) is a later wave.

/**
 * Build the mock project used by the dev wiring.
 *
 * @returns {object} a `model::Project` shaped object
 */
export function buildMockProject() {
    return {
        id: "proj-mock-light",
        name: "mock 智能灯 demo",
        tree: {
            root: {
                name: "MainBoard",
                component_id: "rv-sim",
                children: [
                    {
                        name: "RV32IMC",
                        component_id: "rv32imc",
                        children: [
                            { name: "UART0", component_id: "UART0", children: [] },
                            { name: "GPIO0", component_id: "GPIO0", children: [] },
                        ],
                    },
                ],
            },
        },
        images: [],
        version: {
            schema: 1,
            isa: "RV32IMC",
            toolchain: "mock (dev-only)",
        },
    };
}
