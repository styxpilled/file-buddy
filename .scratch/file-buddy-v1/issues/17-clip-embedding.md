# 17 - CLIP embedding

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
