# Phase 21 — Modern Rendering, Graphics, and Media

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Proposed — current rendering is a deliberately small HTML/CSS pipeline. Images are placeholders; modern layout/compositing, SVG/MathML, Canvas, WebGL/WebGPU and media playback are not browser capabilities yet.

## Objective

Provide the visual, graphics and media platform that ordinary contemporary sites require while retaining one DOM/style/layout/compositor state that humans, automation and AI observe together. The target is standards-based CSS/HTML rendering, Canvas 2D, WebGL 1/2 and a separately negotiated WebGPU path; WebGL follows the [Khronos WebGL specification](https://registry.khronos.org/webgl/specs/latest/1.0/), with extensions exposed only through an explicit capability matrix.

## Decisions

### One style/layout/compositor pipeline

Extend `blueice-css`, layout, paint and raster into a layered rendering pipeline rather than creating special renderer paths for Canvas, SVG, video or DevTools. It must cover CSS custom properties/cascade layers, modern selectors, media/container queries, logical properties, overflow/scrolling, intrinsic sizing, block/inline/flex/grid/table layout, positioning, transforms, opacity, filters, clipping/masking, stacking contexts, compositing, animation/transitions and responsive viewport/color-scheme/reduced-motion behavior. A style/layout snapshot remains generation-bound and exposes declaration-to-pixel provenance.

The compositor consumes immutable display lists/layers from this pipeline. It owns damage tracking, scroll/layer transforms, raster scale, color management, device-pixel ratio and GPU/CPU fallback. `core` remains the owner of document/style state; the frontend remains a presenter, never an alternative compositor or layout engine.

### Resources, typography, SVG and MathML

Add policy-controlled resource decoders for images (`img`, `picture`, `srcset`/`sizes`, CSS images), fonts (`@font-face`, WOFF/WOFF2/OpenType), SVG and MathML. Decoders run in isolated, resource-bounded services; decoded pixels/glyphs/vector display lists cross to the compositor through typed IPC, never arbitrary decoder pointers. Origin/CORS/SRI/CSP/MIME/charset policy is inherited from Phase 20 before decoding.

SVG is namespace-aware XML integrated into DOM/CSS/layout and supports its declared script/event/animation policy through the same BlueJS/event model; it does not obtain a second JavaScript realm or XML fetch resolver. MathML receives semantic layout/accessibility mapping rather than being flattened to text. `@font-face`, font fallback, OpenType shaping, bidi, ruby and locale-sensitive line breaking use one font/text subsystem shared by HTML, SVG, MathML, Canvas, PDF text and native accessibility.

### Canvas, WebGL and WebGPU

`<canvas>` exposes a standards-based Canvas 2D context with pixel buffer ownership, resize/loss semantics, image/video/font inputs, text shaping, transforms, paths, compositing and taint/origin-clean checks. A canvas pixel readback, `toDataURL`, `getImageData` or cross-origin texture operation follows the resource's real origin policy; it is not an AI/DevTools data escape.

Add an isolated `blueice-gpu` service that owns GPU adapter/device/context lifetime and validates every IPC command before submission. WebGL 1 and 2 expose the standard context/state/resource/shader/framebuffer/query APIs, context loss/restoration and only individually tested extensions. Shader compilation/linking, buffer bounds, texture formats, framebuffer completeness, readback, memory quotas, watchdog reset and context loss are enforced at the service boundary. WebGPU is a later sub-slice on the same service and receives a separately versioned feature/limits/adapter capability document; WebGL must never masquerade as WebGPU or vice versa.

Page code only obtains opaque context/resource handles scoped to its document/origin/generation. The GPU service has no DOM, cookie, arbitrary filesystem or network authority. A device crash loses the affected contexts and emits the standard loss event while preserving the rest of the browser.

### Audio, video and timed media

Provide HTML media elements, `AudioContext`/Web Audio graph where selected, Media Source/streams in staged profiles, captions/subtitles and media-session integration. Demuxers/decoders run isolated with byte/time/frame limits and feed decoded frames/audio to the compositor/audio device through bounded queues. Codec support is a published build-time capability matrix with licensing review; encrypted media/DRM and proprietary plugin execution are never silently claimed. Autoplay, capture, microphone/camera, screen capture and output-device selection remain Phase 20 permission decisions with visible human UI.

### AI MCP integration

Phase 12's Web Platform MCP environment exposes `render_*`, `graphics_*` and `media_*` target families: inspect computed style/layout/layers/resource state; trace CSS-to-pixel provenance; capture a bounded canvas/WebGL/media frame; inspect shader/decoder diagnostics, context-loss and performance counters; and observe media lifecycle/caption state. Page script evaluation remains `debug_*`; GPU commands, raw GPU memory, camera/microphone/screen capture, unredacted cross-origin pixels and arbitrary codec input are never direct MCP capabilities.

## Delivery and acceptance

1. Complete modern CSS/layout/compositing and deterministic style/layout/paint differential fixtures before GPU acceleration.
2. Add isolated image/font/SVG/MathML decode/render services and exercise MIME/CORS/SRI/taint/error paths.
3. Ship Canvas 2D, then WebGL 1/2 with conformance subsets, deterministic context loss and GPU watchdog tests; expose WebGPU only after its own native capability matrix passes.
4. Add audio/video/caption playback with isolated decoder regression and accessibility tests.
5. Add the negotiated render/graphics/media MCP adapters and prove their snapshots, errors and frame generations agree with the human frontend and DevTools.

Acceptance requires responsive CSS/grid/animation fixtures; a same- and cross-origin image/font/SVG fixture; a Canvas 2D pixel test; WebGL shader/context-loss/readback/taint fixtures; and captioned audio/video playback. GPU/device loss, hostile image/font/media/shader input, unsupported extension/codec, quota exhaustion and cross-origin readback must fail safely and observably.

## Explicit non-goals

- A GPU command socket, unlimited readback or an AI-controlled screen/camera/microphone capture channel.
- Treating all hardware, graphics extensions, codecs or DRM systems as universally available.
- A second SVG renderer, canvas compositor or font stack outside the document/render pipeline.
