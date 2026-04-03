/** @module Interface host:browser/audio@0.1.0 **/
export function listInputDevices(): Array<string>;
export function listOutputDevices(): Array<string>;
export function defaultInputConfig(): [number, number, number];
export function defaultOutputConfig(): [number, number, number];
export function startCapture(deviceName: string | undefined, sampleRate: number, channels: number): number;
export function readCaptureData(captureId: number): Uint8Array;
export function stopCapture(captureId: number): void;
export function startPlayback(deviceName: string | undefined, sampleRate: number, channels: number): number;
export function enqueuePlayback(playerId: number, data: Uint8Array): void;
export function clearPlayback(playerId: number): void;
export function stopPlayback(playerId: number): void;
