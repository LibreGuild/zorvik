// Audio thread side of microphone streaming (lib/mic.ts): hands the samples to the
// page in blocks of about 5 ms, one Float32Array per channel.
class MicCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.blocks = [];
    this.frames = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (input && input.length) {
      this.blocks.push(input.map((channel) => channel.slice()));
      this.frames += input[0].length;
      if (this.frames >= sampleRate / 200) {
        const channels = this.blocks[0].map((_, c) => {
          const out = new Float32Array(this.frames);
          let at = 0;
          for (const block of this.blocks) {
            out.set(block[c], at);
            at += block[c].length;
          }
          return out;
        });
        this.port.postMessage(channels, channels.map((c) => c.buffer));
        this.blocks = [];
        this.frames = 0;
      }
    }
    return true;
  }
}

registerProcessor("zorvik-mic", MicCapture);
