import './style.css';
import { setupCrossOriginIsolation } from './coi';
import { initApp } from './ui/app';
import { audioEngine } from './audio/engine';

async function main(): Promise<void> {
  await setupCrossOriginIsolation();
  await initApp();
  // AudioContext must be created/resumed from a user gesture.
  window.addEventListener('pointerdown', () => void audioEngine.ensure(), { once: true });
  window.addEventListener('keydown', () => void audioEngine.ensure(), { once: true });
}

void main();
