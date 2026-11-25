// Web Audio engine: AudioContext, the partitioned-convolution AudioWorklet,
// dry/wet routing, live microphone input, and stem playback.

import type { RirBuffers } from '../physics/client';
import { generateStem, type StemKind } from './stems';

const WORKLET_URL = new URL('./convolution-processor.js', import.meta.url);

export interface ConvParams {
  dry: number;
  wet: number;
  predelayMs: number;
  damping: number;
  width: number;
}

export class AudioEngine {
  private ctx: AudioContext | null = null;
  private conv: AudioWorkletNode | null = null;
  private inputGain: GainNode | null = null;
  private micGain: GainNode | null = null;
  private micSource: MediaStreamAudioSourceNode | null = null;
  private micStream: MediaStream | null = null;
  private workletReady = false;
  private params: ConvParams = { dry: 0.5, wet: 0.5, predelayMs: 0, damping: 0.5, width: 1 };
  private lastRir: RirBuffers | null = null;

  async ensure(): Promise<void> {
    if (this.ctx) {
      await this.ctx.resume();
      return;
    }
    const ctx = new AudioContext({ latencyHint: 'interactive' });
    this.ctx = ctx;
    await ctx.audioWorklet.addModule(WORKLET_URL);
    this.workletReady = true;

    this.conv = new AudioWorkletNode(ctx, 'sonophile-convolution', {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });
    this.inputGain = ctx.createGain();
    this.inputGain.gain.value = 1;
    this.inputGain.connect(this.conv);
    this.conv.connect(ctx.destination);

    this.micGain = ctx.createGain();
    this.micGain.gain.value = 0;
    this.micGain.connect(this.inputGain);

    this.pushParams();
    if (this.lastRir) this.setRir(this.lastRir);
    await ctx.resume();
  }

  get sampleRate(): number {
    return this.ctx ? this.ctx.sampleRate : 48000;
  }

  setRir(rir: RirBuffers): void {
    this.lastRir = rir;
    if (!this.conv) return;
    // Transfer stereo RIR to the worklet (click-free crossfade happens there).
    this.conv.port.postMessage(
      { type: 'set-ir', left: rir.stereoL, right: rir.stereoR },
      [rir.stereoL.buffer as ArrayBuffer, rir.stereoR.buffer as ArrayBuffer]
    );
  }

  setParams(p: Partial<ConvParams>): void {
    this.params = { ...this.params, ...p };
    this.pushParams();
  }

  private pushParams(): void {
    if (!this.conv) return;
    this.conv.port.postMessage({
      type: 'param',
      dry: this.params.dry,
      wet: this.params.wet,
      predelay: this.params.predelayMs,
      damping: this.params.damping,
      width: this.params.width,
    });
  }

  async playStem(kind: StemKind): Promise<void> {
    await this.ensure();
    const ctx = this.ctx!;
    const samples = generateStem(kind, ctx.sampleRate);
    const buf = ctx.createBuffer(1, samples.length, ctx.sampleRate);
    buf.copyToChannel(samples as Float32Array<ArrayBuffer>, 0);
    const src = ctx.createBufferSource();
    src.buffer = buf;
    const g = ctx.createGain();
    g.gain.value = 0.9;
    src.connect(g).connect(this.inputGain!);
    src.start();
  }

  async toggleMic(): Promise<boolean> {
    await this.ensure();
    if (this.micSource) {
      this.micSource.disconnect();
      this.micSource = null;
      this.micStream?.getTracks().forEach((t) => t.stop());
      this.micStream = null;
      if (this.micGain) this.micGain.gain.value = 0;
      return false;
    }
    this.micStream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false } });
    this.micSource = this.ctx!.createMediaStreamSource(this.micStream);
    this.micSource.connect(this.micGain!);
    // Input gain with feedback guardrail (monitor muted until user raises it).
    this.micGain!.gain.value = 0;
    return true;
  }

  setMicGain(v: number): void {
    // Clamp to avoid howling feedback while monitoring.
    if (this.micGain) this.micGain.gain.value = Math.max(0, Math.min(1, v));
  }
}

export const audioEngine = new AudioEngine();
