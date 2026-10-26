---
type: llm
focus: trace
weight: 1
---

render_frame no longer clones the device Arc (no Arc::clone or .clone() of the device on the frame path)
