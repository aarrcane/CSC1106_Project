# Quiz Monitoring Notes

The quiz suspicious activity detector is a browser-side monitoring feature. It is an indicator system, not proof of cheating.

## What It Uses

- `getUserMedia()` asks for camera and microphone permission.
- MediaPipe Face Landmarker runs in the browser to detect face landmarks.
- Web Audio `AnalyserNode` checks microphone signal levels for noise spikes.
- Actix receives small JSON event logs and stores them in PostgreSQL/Supabase.

No raw video, image, or audio is stored.

## MediaPipe Assets

The v1 implementation loads MediaPipe from CDN:

- JavaScript/WASM runtime from `cdn.jsdelivr.net`
- Face landmark model from Google's MediaPipe model storage

This matches the existing project style because Bootstrap is also loaded by CDN. If the final demo needs to work without internet access, the MediaPipe WASM/model assets can be downloaded into `static/` and served locally by Actix later.

## Supabase SQL Step

For the existing shared Supabase database, run this file once in the Supabase SQL Editor:

```text
sql/2026-05-29_add_quiz_monitoring_events.sql
```

This creates `quiz_monitoring_events`, indexes it, and enables Row Level Security.

## Demo Flow

1. Login as the demo student.
2. Open `/student/quizzes`.
3. Start the open quiz to reach the instruction/monitoring gate.
4. Click **Start Monitoring** and allow both camera and microphone.
5. Click **Enter Quiz** once monitoring unlocks the entry button.
6. Confirm the quiz timer starts on the quiz page.
7. Trigger activity such as covering the camera, moving out of frame, adding another face, looking away, or making a loud noise.
8. Login as the demo lecturer.
9. Open `/lecturer/quizzes` to review the logged events.
