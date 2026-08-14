// The one number the page and the module both have to know.
//
// The page refuses a file *before* it instantiates anything, so it cannot ask
// the module how large a file the module would hold — by the time it could
// ask, it would already have paid for the answer. So the number lives in two
// places, and `tools/wasm_agreement.mjs` is what stops them drifting: it reads
// this file, reads `MAX_INPUT_BYTES` out of `wasm/src/lib.rs`, and fails if
// they disagree. Then it checks the compiled module really does accept exactly
// this and refuse exactly one byte more.
//
// Peak linear memory is roughly the proof plus the formula plus the store plus
// a megabyte. 32 MiB of proof puts peak at about 48 MB: comfortable in a
// desktop tab, honest on a phone, and far below any published engine limit, so
// that the page is wrong in the direction of refusing something it could have
// checked.

/** The largest formula or proof this page will accept, in bytes. 32 MiB. */
export const MAX_INPUT_BYTES = 33554432;

/** `MAX_INPUT_BYTES` as a human-readable size, for the refusal message. */
export const MAX_INPUT_LABEL = '32 MB';
