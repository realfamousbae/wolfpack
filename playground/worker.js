// Runs a program in the virtual machine of the compiler (spec chapter 10), away from the page,
// so that the page can stop a program that runs too long. The output is posted as it is written.

import init, { run_live } from "./pkg/wolf_wasm.js";

self.onmessage = async ({ data: { source, input } }) => {
  try {
    await init();
    const text = run_live(source, input, (stream, chunk) => self.postMessage({ stream, text: chunk }));
    self.postMessage({ done: true, result: JSON.parse(text) });
  } catch (error) {
    self.postMessage({ done: true, failed: error && error.message ? error.message : String(error) });
  }
};
