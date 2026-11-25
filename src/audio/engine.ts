// Web Audio engine: AudioContext, partitioned-convolution AudioWorklet,
// 3-band EQ, stereo ping-pong delay, chorus, warmth saturation, limiter,
// live AnalyserNode for spectrum/oscilloscope visuals, mic input & stem playback.

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

export interface FxParams {
  eqLowDb: number;     // -12..+12 dB (120 Hz shelf)
  eqMidDb: number;     // -12..+12 dB (1 kHz peaking)
  eqHighDb: number;    // -12..+12 dB (6 kHz shelf)
  delayTimeMs: number; // 20..600 ms
  delayFeedback: number; // 0..0.75
  delayMix: number;    // 0..0.8
  chorusDepth: number; // 0..1
  warmth: number;      // 0..1 soft-clip drive
}

function makeWarmthCurve(amount: number): Float32Array<ArrayBuffer> {
  const n = 1024;
  const curve = new Float32Array(n) as Float32Array<ArrayBuffer>;
  const k = 1 + amount * 14;
  const norm = Math.tanh(k);
  for (let i = 0; i < n; i++) {
    const x = (i * 2) / (n - 1) - 1;
    curve[i] = amount <= 0.001 ? x : Math.tanh(k * x) / norm;
  }
  return curve;
}

export class AudioEngine {
  private ctx: AudioContext | null = null;
  private conv: AudioWorkletNode | null = null;
  private inputGain: GainNode | null = null;
  private micGain: GainNode | null = null;
  private micSource: MediaStreamAudioSourceNode | null = null;
  private micStream: MediaStream | null = null;
  private activeSource: AudioBufferSourceNode | null = null;

  // FX nodes
  private eqLow: BiquadFilterNode | null = null;
  private eqMid: BiquadFilterNode | null = null;
  private eqHigh: BiquadFilterNode | null = null;
  private shaper: WaveShaperNode | null = null;
  private delayNode: DelayNode | null = null;
  private delayFb: GainNode | null = null;
  private delayWet: GainNode | null = null;
  private chorusDelay: DelayNode | null = null;
  private chorusLfo: OscillatorNode | null = null;
  private chorusLfoGain: GainNode | null = null;
  private chorusWet: GainNode | null = null;
  private limiter: DynamicsCompressorNode | null = null;
  private analyser: AnalyserNode | null = null;

  private params: ConvParams = { dry: 0.5, wet: 0.5, predelayMs: 0, damping: 0.5, width: 1 };
  private fx: FxParams = {
    eqLowDb: 0,
    eqMidDb: 0,
    eqHighDb: 0,
    delayTimeMs: 240,
    delayFeedback: 0.3,
    delayMix: 0,
    chorusDepth: 0,
    warmth: 0,
  };
  private lastRir: RirBuffers | null = null;
  private freqBins: Uint8Array<ArrayBuffer> = new Uint8Array(128) as Uint8Array<ArrayBuffer>;
  private timeBins: Uint8Array<ArrayBuffer> = new Uint8Array(256) as Uint8Array<ArrayBuffer>;

  async ensure(): Promise<void> {
    if (this.ctx) {
      await this.ctx.resume();
      return;
    }
    const ctx = new AudioContext({ latencyHint: 'interactive' });
    this.ctx = ctx;

    this.inputGain = ctx.createGain();
    this.inputGain.gain.value = 1;

    // Try AudioWorklet convolution; fallback cleanly to dry pass-through if unavailable.
    let postConvSource: AudioNode = this.inputGain;
    try {
      await ctx.audioWorklet.addModule(WORKLET_URL);
      this.conv = new AudioWorkletNode(ctx, 'sonophile-convolution', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [2],
      });
      this.inputGain.connect(this.conv);
      postConvSource = this.conv;
    } catch (e) {
      console.warn('[audio] AudioWorklet fallback:', e);
    }

    // 3-Band EQ
    this.eqLow = ctx.createBiquadFilter();
    this.eqLow.type = 'lowshelf';
    this.eqLow.frequency.value = 120;

    this.eqMid = ctx.createBiquadFilter();
    this.eqMid.type = 'peaking';
    this.eqMid.frequency.value = 1000;
    this.eqMid.Q.value = 1.0;

    this.eqHigh = ctx.createBiquadFilter();
    this.eqHigh.type = 'highshelf';
    this.eqHigh.frequency.value = 6000;

    // Warmth saturation
    this.shaper = ctx.createWaveShaper();
    this.shaper.oversample = '2x';
    this.shaper.curve = makeWarmthCurve(0);

    // Parallel Delay
    this.delayNode = ctx.createDelay(1.0);
    this.delayFb = ctx.createGain();
    this.delayWet = ctx.createGain();
    this.delayNode.connect(this.delayFb).connect(this.delayNode);
    this.delayNode.connect(this.delayWet);

    // Parallel Chorus
    this.chorusDelay = ctx.createDelay(0.1);
    this.chorusDelay.delayTime.value = 0.015;
    this.chorusLfo = ctx.createOscillator();
    this.chorusLfo.frequency.value = 1.4;
    this.chorusLfoGain = ctx.createGain();
    this.chorusLfoGain.gain.value = 0;
    this.chorusWet = ctx.createGain();
    this.chorusWet.gain.value = 0;
    this.chorusLfo.connect(this.chorusLfoGain).connect(this.chorusDelay.delayTime);
    this.chorusDelay.connect(this.chorusWet);
    this.chorusLfo.start();

    // Master Limiter + Analyser
    this.limiter = ctx.createDynamicsCompressor();
    this.limiter.threshold.value = -3;
    this.limiter.ratio.value = 12;
    this.limiter.attack.value = 0.003;
    this.limiter.release.value = 0.15;

    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 256;
    this.analyser.smoothingTimeConstant = 0.8;
    this.freqBins = new Uint8Array(this.analyser.frequencyBinCount) as Uint8Array<ArrayBuffer>;
    this.timeBins = new Uint8Array(this.analyser.fftSize) as Uint8Array<ArrayBuffer>;

    // Connect chain: postConv -> EQ -> Shaper -> (Direct + Delay + Chorus) -> Limiter -> Analyser -> Dest
    postConvSource.connect(this.eqLow).connect(this.eqMid).connect(this.eqHigh).connect(this.shaper);
    this.shaper.connect(this.limiter);
    this.shaper.connect(this.delayNode);
    this.shaper.connect(this.chorusDelay);
    this.delayWet.connect(this.limiter);
    this.chorusWet.connect(this.limiter);
    this.limiter.connect(this.analyser).connect(ctx.destination);

    this.micGain = ctx.createGain();
    this.micGain.gain.value = 0;
    this.micGain.connect(this.inputGain);

    this.pushParams();
    this.pushFx();
    if (this.lastRir) this.setRir(this.lastRir);
    await ctx.resume();
  }

  get sampleRate(): number {
    return this.ctx ? this.ctx.sampleRate : 48000;
  }

  setRir(rir: RirBuffers): void {
    this.lastRir = rir;
    if (!this.conv) return;
    // Clone buffers before transfer so `this.lastRir` is never neutered.
    const lCopy = rir.stereoL.slice();
    const rCopy = rir.stereoR.slice();
    this.conv.port.postMessage(
      { type: 'set-ir', left: lCopy, right: rCopy },
      [lCopy.buffer as ArrayBuffer, rCopy.buffer as ArrayBuffer]
    );
  }

  setParams(p: Partial<ConvParams>): void {
    this.params = { ...this.params, ...p };
    this.pushParams();
  }

  setFx(p: Partial<FxParams>): void {
    this.fx = { ...this.fx, ...p };
    this.pushFx();
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

  private pushFx(): void {
    if (!this.ctx) return;
    const now = this.ctx.currentTime;
    this.eqLow?.gain.setTargetAtTime(this.fx.eqLowDb, now, 0.02);
    this.eqMid?.gain.setTargetAtTime(this.fx.eqMidDb, now, 0.02);
    this.eqHigh?.gain.setTargetAtTime(this.fx.eqHighDb, now, 0.02);
    this.delayNode?.delayTime.setTargetAtTime(Math.max(0.02, this.fx.delayTimeMs / 1000), now, 0.02);
    this.delayFb?.gain.setTargetAtTime(Math.min(0.75, Math.max(0, this.fx.delayFeedback)), now, 0.02);
    this.delayWet?.gain.setTargetAtTime(Math.min(0.85, Math.max(0, this.fx.delayMix)), now, 0.02);
    this.chorusLfoGain?.gain.setTargetAtTime(this.fx.chorusDepth * 0.006, now, 0.02);
    this.chorusWet?.gain.setTargetAtTime(this.fx.chorusDepth * 0.55, now, 0.02);
    if (this.shaper) {
      this.shaper.curve = makeWarmthCurve(this.fx.warmth);
    }
  }

  getVisualizerData(): { freq: Uint8Array; time: Uint8Array; rms: number } | null {
    if (!this.analyser) return null;
    this.analyser.getByteFrequencyData(this.freqBins);
    this.analyser.getByteTimeDomainData(this.timeBins);
    let sum = 0;
    for (let i = 0; i < this.timeBins.length; i++) {
      const v = (this.timeBins[i] - 128) / 128;
      sum += v * v;
    }
    return { freq: this.freqBins, time: this.timeBins, rms: Math.sqrt(sum / this.timeBins.length) };
  }

  stopStem(): void {
    if (this.activeSource) {
      try { this.activeSource.stop(); } catch { /* already stopped */ }
      this.activeSource.disconnect();
      this.activeSource = null;
    }
  }

  async playStem(kind: StemKind, loop = false): Promise<void> {
    await this.ensure();
    this.stopStem();
    const ctx = this.ctx!;
    const samples = generateStem(kind, ctx.sampleRate);
    const buf = ctx.createBuffer(1, samples.length, ctx.sampleRate);
    buf.copyToChannel(samples as Float32Array<ArrayBuffer>, 0);
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.loop = loop;
    const g = ctx.createGain();
    g.gain.value = 0.9;
    src.connect(g).connect(this.inputGain!);
    src.onended = () => {
      if (this.activeSource === src) this.activeSource = null;
    };
    this.activeSource = src;
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
    this.micGain!.gain.value = 0.5;
    return true;
  }

  setMicGain(v: number): void {
    if (this.micGain) this.micGain.gain.value = Math.max(0, Math.min(1, v));
  }
}

export const audioEngine = new AudioEngine();
