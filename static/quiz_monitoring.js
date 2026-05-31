const root = document.getElementById("quiz-monitor-root");

if (root) {
  const eventUrl = root.dataset.eventUrl;
  const readyUrl = root.dataset.readyUrl;
  const autoStart = root.dataset.autostart === "true";
  const video = document.getElementById("monitorVideo");
  const startButton = document.getElementById("startMonitoringButton");
  const eventLog = document.getElementById("monitorEventLog");
  const overallStatus = document.getElementById("monitorOverallStatus");
  const enterQuizButton = document.getElementById("enterQuizButton");
  const submitButton = document.getElementById("submitQuizAttempt");
  const quizForm = document.getElementById("quizAttemptForm");
  const quizLockAlert = document.getElementById("quizLockAlert");
  const quizLockedPanel = document.getElementById("quizLockedPanel");

  const statusElements = {
    camera: {
      dot: document.getElementById("cameraStatusDot"),
      text: document.getElementById("cameraStatusText"),
    },
    microphone: {
      dot: document.getElementById("microphoneStatusDot"),
      text: document.getElementById("microphoneStatusText"),
    },
    face: {
      dot: document.getElementById("faceStatusDot"),
      text: document.getElementById("faceStatusText"),
    },
    noise: {
      dot: document.getElementById("noiseStatusDot"),
      text: document.getElementById("noiseStatusText"),
    },
  };

  let faceLandmarker = null;
  let cameraStream = null;
  let microphoneStream = null;
  let audioContext = null;
  let analyser = null;
  let audioData = null;
  let animationFrameId = null;
  let monitoring = false;
  let firstLogRendered = false;
  let faceMonitoringReady = false;

  const detectionState = {
    faceMissingSince: null,
    multipleFacesSince: null,
    lookingAwaySince: null,
    noiseSpikeSince: null,
    hadFace: false,
    lastEventAt: new Map(),
  };

  const cooldownMs = {
    face_missing: 10000,
    face_restored: 10000,
    multiple_faces: 10000,
    looking_away: 10000,
    noise_spike: 10000,
    monitoring_error: 5000,
  };

  startButton?.addEventListener("click", startMonitoring);
  lockQuiz("Camera and microphone monitoring must be enabled before this quiz can be answered.");

  submitButton?.addEventListener("click", () => {
    window.alert("Mock quiz attempt submitted. Monitoring events are available for lecturer review.");
  });

  enterQuizButton?.addEventListener("click", (event) => {
    if (enterQuizButton.getAttribute("aria-disabled") === "true") {
      event.preventDefault();
    }
  });

  if (autoStart) {
    startMonitoring();
  }

  function setOverallStatus(text, className) {
    if (!overallStatus) {
      return;
    }

    overallStatus.className = `badge ${className} rounded-pill`;
    overallStatus.textContent = text;
  }

  function setStatus(type, text, level) {
    const element = statusElements[type];
    if (!element) {
      return;
    }

    const classByLevel = {
      idle: "bg-secondary",
      ok: "bg-success",
      warn: "bg-warning",
      danger: "bg-danger",
    };

    element.dot.className = `monitor-dot ${classByLevel[level] || classByLevel.idle}`;
    element.text.textContent = text;
  }

  async function startMonitoring() {
    if (monitoring) {
      return;
    }

    if (!navigator.mediaDevices?.getUserMedia) {
      setOverallStatus("Unavailable", "bg-danger");
      logEvent("monitoring_error", "critical", "Browser does not support getUserMedia.");
      return;
    }

    monitoring = true;
    startButton.disabled = true;
    setOverallStatus("Starting", "bg-warning text-dark");

    await requestCamera();
    await requestMicrophone();

    if (!cameraStream || !microphoneStream) {
      lockQuiz("Camera and microphone are required. Please allow both permissions to answer the quiz.");
      setOverallStatus("Permissions required", "bg-danger");
      startButton.disabled = false;
      monitoring = false;
      stopStream(cameraStream);
      stopStream(microphoneStream);
      cameraStream = null;
      microphoneStream = null;
      video.srcObject = null;
      return;
    }

    if (cameraStream) {
      try {
        await loadFaceLandmarker();
        faceMonitoringReady = true;
        setStatus("face", "Face checking", "ok");
      } catch (error) {
        faceMonitoringReady = false;
        setStatus("face", "Face unavailable", "danger");
        logEvent("monitoring_error", "critical", `MediaPipe failed to load: ${error.message}`);
      }
    }

    if (!faceMonitoringReady) {
      lockQuiz("Face monitoring could not start. Check your internet connection and try again.");
      setOverallStatus("Face monitor required", "bg-danger");
      startButton.disabled = false;
      monitoring = false;
      stopStream(cameraStream);
      stopStream(microphoneStream);
      cameraStream = null;
      microphoneStream = null;
      video.srcObject = null;
      return;
    }

    logEvent("monitoring_started", "info", "Student started quiz activity monitoring.");
    setOverallStatus("Running", "bg-success");
    const readyMarked = await markMonitoringReady();
    if (!readyMarked) {
      lockQuiz("Monitoring started, but the quiz could not be unlocked. Try again.");
      setOverallStatus("Unlock failed", "bg-danger");
      return;
    }
    unlockQuiz();
    animationFrameId = window.requestAnimationFrame(detectLoop);
  }

  async function requestCamera() {
    try {
      cameraStream = await navigator.mediaDevices.getUserMedia({
        video: {
          width: { ideal: 640 },
          height: { ideal: 480 },
          facingMode: "user",
        },
        audio: false,
      });
      video.srcObject = cameraStream;
      await video.play();
      setStatus("camera", "Camera active", "ok");
    } catch (error) {
      setStatus("camera", "Camera denied", "danger");
      logEvent("camera_permission_denied", "warning", error.message);
    }
  }

  async function requestMicrophone() {
    try {
      microphoneStream = await navigator.mediaDevices.getUserMedia({
        audio: true,
        video: false,
      });

      audioContext = new AudioContext();
      const source = audioContext.createMediaStreamSource(microphoneStream);
      analyser = audioContext.createAnalyser();
      analyser.fftSize = 1024;
      audioData = new Uint8Array(analyser.fftSize);
      source.connect(analyser);
      setStatus("microphone", "Mic active", "ok");
      setStatus("noise", "Noise normal", "ok");
    } catch (error) {
      setStatus("microphone", "Mic denied", "danger");
      setStatus("noise", "Noise unavailable", "warn");
      logEvent("microphone_permission_denied", "warning", error.message);
    }
  }

  async function loadFaceLandmarker() {
    if (faceLandmarker) {
      return;
    }

    const visionModule = await import("https://cdn.jsdelivr.net/npm/@mediapipe/tasks-vision@0.10.14");
    const vision = visionModule.default || visionModule;
    const { FaceLandmarker, FilesetResolver } = vision;
    const filesetResolver = await FilesetResolver.forVisionTasks(
      "https://cdn.jsdelivr.net/npm/@mediapipe/tasks-vision@0.10.14/wasm",
    );

    faceLandmarker = await FaceLandmarker.createFromOptions(filesetResolver, {
      baseOptions: {
        modelAssetPath:
          "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/latest/face_landmarker.task",
        delegate: "CPU",
      },
      outputFaceBlendshapes: true,
      runningMode: "VIDEO",
      numFaces: 2,
    });
  }

  function detectLoop() {
    if (!monitoring) {
      return;
    }

    const now = performance.now();
    detectFaces(now);
    detectNoise(now);
    animationFrameId = window.requestAnimationFrame(detectLoop);
  }

  function detectFaces(now) {
    if (!faceLandmarker || !video || video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
      return;
    }

    const result = faceLandmarker.detectForVideo(video, now);
    const faces = result.faceLandmarks || [];

    if (faces.length === 0) {
      setStatus("face", "Face missing", "danger");
      if (detectionState.hadFace) {
        detectionState.hadFace = false;
      }
      if (!detectionState.faceMissingSince) {
        detectionState.faceMissingSince = now;
      }
      if (now - detectionState.faceMissingSince > 2500) {
        logEvent("face_missing", "critical", "No face detected for more than 2.5 seconds.");
      }
      return;
    }

    if (!detectionState.hadFace && detectionState.faceMissingSince) {
      logEvent("face_restored", "info", "Face detected again after being missing.");
    }
    detectionState.hadFace = true;
    detectionState.faceMissingSince = null;

    if (faces.length > 1) {
      setStatus("face", "Multiple faces", "danger");
      if (!detectionState.multipleFacesSince) {
        detectionState.multipleFacesSince = now;
      }
      if (now - detectionState.multipleFacesSince > 1500) {
        logEvent("multiple_faces", "critical", "More than one face detected in the camera frame.");
      }
      return;
    }

    detectionState.multipleFacesSince = null;

    if (isLookingAway(faces[0])) {
      setStatus("face", "Looking away", "warn");
      if (!detectionState.lookingAwaySince) {
        detectionState.lookingAwaySince = now;
      }
      if (now - detectionState.lookingAwaySince > 2500) {
        logEvent("looking_away", "warning", "Head or eye position suggests the student looked away.");
      }
      return;
    }

    detectionState.lookingAwaySince = null;
    setStatus("face", "Face centered", "ok");
  }

  function isLookingAway(landmarks) {
    const nose = landmarks[1];
    const leftCheek = landmarks[234];
    const rightCheek = landmarks[454];
    const forehead = landmarks[10];
    const chin = landmarks[152];

    if (!nose || !leftCheek || !rightCheek || !forehead || !chin) {
      return false;
    }

    const faceWidth = Math.max(Math.abs(rightCheek.x - leftCheek.x), 0.001);
    const faceHeight = Math.max(Math.abs(chin.y - forehead.y), 0.001);
    const faceCenterX = (leftCheek.x + rightCheek.x) / 2;
    const faceCenterY = (forehead.y + chin.y) / 2;
    const headTurn = Math.abs((nose.x - faceCenterX) / faceWidth) > 0.16;
    const headDown = (nose.y - faceCenterY) / faceHeight > 0.18;

    const leftIris = landmarks[468];
    const rightIris = landmarks[473];
    const leftEyeOuter = landmarks[33];
    const leftEyeInner = landmarks[133];
    const rightEyeInner = landmarks[362];
    const rightEyeOuter = landmarks[263];

    let irisAway = false;
    if (leftIris && rightIris && leftEyeOuter && leftEyeInner && rightEyeInner && rightEyeOuter) {
      const leftEyeWidth = Math.max(Math.abs(leftEyeInner.x - leftEyeOuter.x), 0.001);
      const rightEyeWidth = Math.max(Math.abs(rightEyeOuter.x - rightEyeInner.x), 0.001);
      const leftRatio = (leftIris.x - leftEyeOuter.x) / leftEyeWidth;
      const rightRatio = (rightIris.x - rightEyeInner.x) / rightEyeWidth;
      const averageRatio = (leftRatio + rightRatio) / 2;
      irisAway = averageRatio < 0.28 || averageRatio > 0.72;
    }

    return headTurn || headDown || irisAway;
  }

  function detectNoise(now) {
    if (!analyser || !audioData) {
      return;
    }

    analyser.getByteTimeDomainData(audioData);
    let sumSquares = 0;

    for (const value of audioData) {
      const normalized = (value - 128) / 128;
      sumSquares += normalized * normalized;
    }

    const rms = Math.sqrt(sumSquares / audioData.length);

    if (rms > 0.07) {
      setStatus("noise", "Noise spike", "warn");
      if (!detectionState.noiseSpikeSince) {
        detectionState.noiseSpikeSince = now;
      }
      if (now - detectionState.noiseSpikeSince > 350) {
        logEvent("noise_spike", "warning", `Sustained room noise or voice detected. RMS=${rms.toFixed(3)}`);
      }
      return;
    }

    detectionState.noiseSpikeSince = null;
    setStatus("noise", "Noise normal", "ok");
  }

  function stopMonitoring() {
    monitoring = false;
    startButton.disabled = false;
    setOverallStatus("Stopped", "bg-secondary");

    if (animationFrameId) {
      window.cancelAnimationFrame(animationFrameId);
      animationFrameId = null;
    }

    stopStream(cameraStream);
    stopStream(microphoneStream);
    cameraStream = null;
    microphoneStream = null;
    video.srcObject = null;

    if (audioContext) {
      audioContext.close();
      audioContext = null;
    }

    analyser = null;
    audioData = null;
    faceMonitoringReady = false;
    setStatus("camera", "Camera stopped", "idle");
    setStatus("microphone", "Mic stopped", "idle");
    setStatus("face", "Face stopped", "idle");
    setStatus("noise", "Noise stopped", "idle");
    lockQuiz("Monitoring stopped. Start camera and microphone monitoring to continue answering.");
  }

  function stopStream(stream) {
    stream?.getTracks().forEach((track) => track.stop());
  }

  function lockQuiz(message) {
    quizForm?.querySelectorAll("input, select, textarea").forEach((control) => {
      control.disabled = true;
    });

    if (submitButton) {
      submitButton.disabled = true;
    }

    if (enterQuizButton) {
      enterQuizButton.classList.add("disabled");
      enterQuizButton.setAttribute("aria-disabled", "true");
      enterQuizButton.setAttribute("tabindex", "-1");
    }

    if (quizLockAlert) {
      quizLockAlert.classList.remove("d-none", "alert-success");
      quizLockAlert.classList.add("alert-warning");
      quizLockAlert.innerHTML = `<i class="bi bi-lock-fill me-2"></i>${escapeHtml(message)}`;
    }

    quizForm?.classList.add("d-none");
    quizForm?.setAttribute("hidden", "hidden");
    if (quizForm) {
      quizForm.style.display = "none";
    }
    quizLockedPanel?.classList.remove("d-none");
    quizLockedPanel?.removeAttribute("hidden");
    if (quizLockedPanel) {
      quizLockedPanel.style.display = "";
    }
  }

  function unlockQuiz() {
    quizForm?.querySelectorAll("input, select, textarea").forEach((control) => {
      control.disabled = false;
    });

    if (submitButton) {
      submitButton.disabled = false;
    }

    if (enterQuizButton) {
      enterQuizButton.classList.remove("disabled");
      enterQuizButton.setAttribute("aria-disabled", "false");
      enterQuizButton.removeAttribute("tabindex");
    }

    if (quizLockAlert) {
      quizLockAlert.classList.remove("alert-warning");
      quizLockAlert.classList.add("alert-success");
      quizLockAlert.innerHTML = enterQuizButton
        ? '<i class="bi bi-unlock-fill me-2"></i>Monitoring is active. You may enter the quiz.'
        : '<i class="bi bi-unlock-fill me-2"></i>Monitoring is active. The quiz is now unlocked.';
    }

    quizLockedPanel?.classList.add("d-none");
    quizLockedPanel?.setAttribute("hidden", "hidden");
    if (quizLockedPanel) {
      quizLockedPanel.style.display = "none";
    }
    quizForm?.classList.remove("d-none");
    quizForm?.removeAttribute("hidden");
    if (quizForm) {
      quizForm.style.display = "";
    }
  }

  async function markMonitoringReady() {
    if (!readyUrl) {
      return true;
    }

    try {
      const response = await fetch(readyUrl, {
        method: "POST",
        credentials: "same-origin",
      });

      if (!response.ok) {
        renderEvent("monitoring_error", "warning", `Server rejected unlock with status ${response.status}.`);
        return false;
      }

      return true;
    } catch (error) {
      renderEvent("monitoring_error", "warning", `Could not unlock quiz: ${error.message}`);
      return false;
    }
  }

  async function logEvent(eventType, severity, details) {
    const now = performance.now();
    const lastAt = detectionState.lastEventAt.get(eventType) || 0;
    const cooldown = cooldownMs[eventType] || 0;

    if (cooldown > 0 && now - lastAt < cooldown) {
      return;
    }

    detectionState.lastEventAt.set(eventType, now);
    renderEvent(eventType, severity, details);

    try {
      const response = await fetch(eventUrl, {
        method: "POST",
        credentials: "same-origin",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          event_type: eventType,
          severity,
          details,
        }),
      });

      if (!response.ok) {
        renderEvent("monitoring_error", "warning", `Server rejected event with status ${response.status}.`);
      }
    } catch (error) {
      renderEvent("monitoring_error", "warning", `Could not send event to server: ${error.message}`);
    }
  }

  function renderEvent(eventType, severity, details) {
    if (!eventLog) {
      return;
    }

    if (!firstLogRendered) {
      eventLog.innerHTML = "";
      firstLogRendered = true;
    }

    const row = document.createElement("div");
    row.className = "quiz-monitor-log-row";
    row.innerHTML = `
      <div class="d-flex align-items-center justify-content-between gap-2">
        <span class="fw-semibold">${formatEventType(eventType)}</span>
        <span class="badge ${badgeClass(severity)} rounded-pill">${severity}</span>
      </div>
      <div class="small text-muted">${new Date().toLocaleTimeString()} - ${escapeHtml(details || "")}</div>
    `;
    eventLog.prepend(row);
  }

  function formatEventType(eventType) {
    return eventType
      .split("_")
      .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
      .join(" ");
  }

  function badgeClass(severity) {
    if (severity === "critical") {
      return "bg-danger";
    }
    if (severity === "warning") {
      return "bg-warning text-dark";
    }
    return "bg-secondary";
  }

  function escapeHtml(value) {
    return value
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }
}
