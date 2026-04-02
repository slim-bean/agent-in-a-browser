/**
 * Browser Audio Shim.
 *
 * Implements the host-side of `host:browser/audio@0.1.0`.
 * The WASM component calls audio functions and this shim routes them
 * to the Web Audio API (AudioContext, getUserMedia, etc.).
 *
 * Capture: Uses MediaStream + ScriptProcessorNode to extract PCM data.
 * Playback: Uses AudioContext + AudioBufferSourceNode queue.
 */

// ============================================================================
// Types
// ============================================================================

interface CaptureSession {
    stream: MediaStream;
    context: AudioContext;
    source: MediaStreamAudioSourceNode;
    processor: ScriptProcessorNode;
    buffer: Int16Array[];
    peak: number;
}

interface PlaybackSession {
    context: AudioContext;
    sampleRate: number;
    channels: number;
    queue: Int16Array[];
    isPlaying: boolean;
    nextStartTime: number;
}

// ============================================================================
// State
// ============================================================================

let nextCaptureId = 1;
let nextPlayerId = 1;
const captures = new Map<number, CaptureSession>();
const playbacks = new Map<number, PlaybackSession>();

// ============================================================================
// Device enumeration
// ============================================================================

/**
 * List available audio input device names.
 */
export async function listInputDevices(): Promise<string[]> {
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.enumerateDevices) {
        return ['Default Microphone'];
    }
    try {
        // Request permission first to get device labels
        try {
            const tempStream = await navigator.mediaDevices.getUserMedia({ audio: true });
            tempStream.getTracks().forEach(t => t.stop());
        } catch (_e: unknown) {
            // Permission denied — return generic names
        }
        const devices = await navigator.mediaDevices.enumerateDevices();
        const inputDevices = devices
            .filter(d => d.kind === 'audioinput')
            .map(d => d.label || `Microphone ${d.deviceId.substring(0, 8)}`);
        return inputDevices.length > 0 ? inputDevices : ['Default Microphone'];
    } catch (_e: unknown) {
        return ['Default Microphone'];
    }
}

/**
 * List available audio output device names.
 */
export async function listOutputDevices(): Promise<string[]> {
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.enumerateDevices) {
        return ['Default Speaker'];
    }
    try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        const outputDevices = devices
            .filter(d => d.kind === 'audiooutput')
            .map(d => d.label || `Speaker ${d.deviceId.substring(0, 8)}`);
        return outputDevices.length > 0 ? outputDevices : ['Default Speaker'];
    } catch (_e: unknown) {
        return ['Default Speaker'];
    }
}

/**
 * Get the default audio configuration for input.
 * Returns [sampleRate, channels, sampleFormat] where sampleFormat: 0=I16, 1=U16, 2=F32.
 */
export function defaultInputConfig(): [number, number, number] {
    // Browser audio capture: 24kHz mono F32
    return [24000, 1, 2];
}

/**
 * Get the default audio configuration for output.
 */
export function defaultOutputConfig(): [number, number, number] {
    // Browser audio playback: 48kHz stereo F32
    return [48000, 2, 2];
}

// ============================================================================
// Capture
// ============================================================================

/**
 * Start capturing audio from a device.
 */
export async function startCapture(
    deviceName: string | undefined,
    sampleRate: number,
    channels: number,
): Promise<number> {
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.getUserMedia) {
        throw new Error('getUserMedia not available');
    }

    // Find device ID by name if specified
    let deviceId: string | undefined;
    if (deviceName) {
        try {
            const devices = await navigator.mediaDevices.enumerateDevices();
            const match = devices.find(
                d => d.kind === 'audioinput' && d.label === deviceName,
            );
            if (match) {
                deviceId = match.deviceId;
            }
        } catch (_e: unknown) {
            // Fall through to use default
        }
    }

    const constraints: MediaStreamConstraints = {
        audio: {
            ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
            sampleRate: { ideal: sampleRate },
            channelCount: { ideal: channels },
            echoCancellation: false,
            noiseSuppression: false,
            autoGainControl: false,
        },
    };

    const stream = await navigator.mediaDevices.getUserMedia(constraints);
    const context = new AudioContext({ sampleRate });
    const source = context.createMediaStreamSource(stream);

    // ScriptProcessorNode is deprecated but widely supported and simpler
    // than AudioWorklet for this use case. Buffer size of 4096 gives ~85ms
    // at 48kHz which is fine for voice.
    const bufferSize = 4096;
    const processor = context.createScriptProcessor(bufferSize, channels, channels);

    const captureId = nextCaptureId++;
    const session: CaptureSession = {
        stream,
        context,
        source,
        processor,
        buffer: [],
        peak: 0,
    };

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    processor.onaudioprocess = (event: any) => {
        const inputData = event.inputBuffer.getChannelData(0);

        // Track peak level
        let peak = 0;
        for (let i = 0; i < inputData.length; i++) {
            const abs = Math.abs(inputData[i]);
            if (abs > peak) peak = abs;
        }
        session.peak = Math.min(Math.floor(peak * 32767), 32767);

        // Convert f32 samples to i16 LE bytes
        const samples = new Int16Array(inputData.length);
        for (let i = 0; i < inputData.length; i++) {
            const clamped = Math.max(-1, Math.min(1, inputData[i]));
            samples[i] = Math.floor(clamped * 32767);
        }

        session.buffer.push(samples);
    };

    source.connect(processor);
    processor.connect(context.destination);

    captures.set(captureId, session);
    return captureId;
}

/**
 * Read captured PCM audio data (16-bit LE).
 */
export function readCaptureData(captureId: number): Uint8Array {
    const session = captures.get(captureId);
    if (!session) {
        throw new Error(`capture session ${captureId} not found`);
    }

    if (session.buffer.length === 0) {
        return new Uint8Array(0);
    }

    // Drain buffer
    const chunks = session.buffer.splice(0);
    let totalSamples = 0;
    for (const chunk of chunks) {
        totalSamples += chunk.length;
    }

    // Convert i16 samples to LE bytes
    const result = new Uint8Array(totalSamples * 2);
    let offset = 0;
    for (const chunk of chunks) {
        for (let i = 0; i < chunk.length; i++) {
            const sample = chunk[i];
            result[offset++] = sample & 0xff;
            result[offset++] = (sample >> 8) & 0xff;
        }
    }

    return result;
}

/**
 * Get the current peak level for a running capture.
 */
export function getCapturePeak(captureId: number): number {
    const session = captures.get(captureId);
    if (!session) return 0;
    return session.peak;
}

/**
 * Stop a running capture.
 */
export function stopCapture(captureId: number): void {
    const session = captures.get(captureId);
    if (!session) {
        throw new Error(`capture session ${captureId} not found`);
    }

    session.processor.disconnect();
    session.source.disconnect();
    session.stream.getTracks().forEach(t => t.stop());
    void session.context.close();

    captures.delete(captureId);
}

// ============================================================================
// Playback
// ============================================================================

/**
 * Start audio playback on a device.
 */
export function startPlayback(
    _deviceName: string | undefined,
    sampleRate: number,
    channels: number,
): number {
    const context = new AudioContext({ sampleRate });
    const playerId = nextPlayerId++;

    playbacks.set(playerId, {
        context,
        sampleRate,
        channels,
        queue: [],
        isPlaying: false,
        nextStartTime: 0,
    });

    return playerId;
}

/**
 * Enqueue PCM audio data (16-bit LE) for playback.
 */
export function enqueuePlayback(playerId: number, data: Uint8Array): void {
    const session = playbacks.get(playerId);
    if (!session) {
        throw new Error(`playback session ${playerId} not found`);
    }

    if (data.length < 2) return;

    // Convert LE bytes to i16 samples
    const numSamples = Math.floor(data.length / 2);
    const samples = new Int16Array(numSamples);
    for (let i = 0; i < numSamples; i++) {
        samples[i] = data[i * 2] | (data[i * 2 + 1] << 8);
    }

    // Convert i16 to f32 and create an AudioBuffer
    const numFrames = Math.floor(numSamples / session.channels);
    if (numFrames === 0) return;

    const audioBuffer = session.context.createBuffer(
        session.channels,
        numFrames,
        session.sampleRate,
    );

    for (let ch = 0; ch < session.channels; ch++) {
        const channelData = audioBuffer.getChannelData(ch);
        for (let i = 0; i < numFrames; i++) {
            const sampleIdx = i * session.channels + ch;
            channelData[i] = sampleIdx < samples.length
                ? samples[sampleIdx] / 32767
                : 0;
        }
    }

    // Schedule playback
    const source = session.context.createBufferSource();
    source.buffer = audioBuffer;
    source.connect(session.context.destination);

    const now = session.context.currentTime;
    const startTime = Math.max(session.nextStartTime, now);
    source.start(startTime);

    session.nextStartTime = startTime + audioBuffer.duration;
}

/**
 * Clear the playback buffer.
 */
export function clearPlayback(playerId: number): void {
    const session = playbacks.get(playerId);
    if (!session) {
        throw new Error(`playback session ${playerId} not found`);
    }

    // Reset the scheduling time so new audio starts immediately
    session.nextStartTime = 0;
    session.queue.length = 0;
}

/**
 * Stop playback.
 */
export function stopPlayback(playerId: number): void {
    const session = playbacks.get(playerId);
    if (!session) {
        throw new Error(`playback session ${playerId} not found`);
    }

    void session.context.close();
    playbacks.delete(playerId);
}
