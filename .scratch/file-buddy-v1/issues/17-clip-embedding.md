# 17 - CLIP embedding

Status: done

Using CLIP for semantic image search is an interesting avenue that can prove just as meaningful as OCR.

The spec lists semantic search as a v1 non-goal. The research can start now. The implementation waits for 16 and the v2 goals review.

## Work

- Spawn an agent to do research on how to best achieve semantic image search.
  - Developer suggestion: `open_clip_inference` from crates.io, user downloads open-clip model manually, use ONNX runtime.
  - Evaluate the costs: dependency size, build-time downloads, CPU against GPU (DirectML).
  - Present the results.
- After the path has been chosen, implement it as a stage in the job queue.

## Additional considerations

- A fifth ranking signal, ranked above OCR. The other four are bm25 column weights, so the research must say how a vector similarity score merges with them.
- Embed several frames per video, sampled the same way as OCR.

## Comments

### 2026-09-23, agent

The research is done and the path is chosen. The runtime is `ort` with
DirectML and a vendored `onnxruntime.dll`. The model is MobileCLIP2-S2, with our
own preprocessing. Search compares against every vector in memory and merges
with bm25 by weighted reciprocal rank fusion. The facts are in
`.scratch/file-buddy-v2/research.md` and the design is in
`.scratch/file-buddy-v2/spec.md`. The implementation continues as v2 issues 20
to 22.
