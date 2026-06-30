(function () {
  const MAX_IMAGE_DIMENSION = 900;
  const JPEG_QUALITY = 0.72;
  const MIN_COMPRESS_BYTES = 120 * 1024;

  const processingForms = new WeakSet();

  function submitButtonsFor(form) {
    if (!form.id) {
      return [];
    }
    return Array.from(
      document.querySelectorAll(`button[type="submit"][form="${form.id}"]`)
    );
  }

  function resetForumForm(form) {
    if (!form) {
      return;
    }
    form.reset();
    delete form.dataset.forumImagesCompressed;
    processingForms.delete(form);
    submitButtonsFor(form).forEach((button) => {
      button.disabled = false;
      if (button.dataset.originalText) {
        button.innerHTML = button.dataset.originalText;
      }
    });
  }

  function markSubmitting(form) {
    submitButtonsFor(form).forEach((button) => {
      if (!button.dataset.originalText) {
        button.dataset.originalText = button.innerHTML;
      }
      button.disabled = true;
      button.textContent = "Posting...";
    });
  }

  function isForumUploadForm(form) {
    return (
      form &&
      form.matches('form[enctype="multipart/form-data"]') &&
      form.querySelector('input[type="file"][name="images"]')
    );
  }

  function hasSelectedImages(form) {
    return Array.from(
      form.querySelectorAll('input[type="file"][name="images"]')
    ).some((input) => input.files && input.files.length > 0);
  }

  function canCompress(file) {
    return (
      file &&
      (file.type === "image/jpeg" || file.type === "image/png") &&
      (file.size >= MIN_COMPRESS_BYTES || file.type === "image/png")
    );
  }

  function renamedAsJpeg(filename) {
    return filename.replace(/\.[^.]+$/, "") + ".jpg";
  }

  async function compressImage(file) {
    if (!canCompress(file) || !window.createImageBitmap) {
      return file;
    }

    let bitmap;
    try {
      bitmap = await createImageBitmap(file);
    } catch (_) {
      return file;
    }

    const scale = Math.min(
      1,
      MAX_IMAGE_DIMENSION / Math.max(bitmap.width, bitmap.height)
    );
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;

    const context = canvas.getContext("2d");
    if (!context) {
      bitmap.close?.();
      return file;
    }

    context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close?.();

    const blob = await new Promise((resolve) => {
      canvas.toBlob(resolve, "image/jpeg", JPEG_QUALITY);
    });

    if (!blob || blob.size >= file.size) {
      return file;
    }

    return new File([blob], renamedAsJpeg(file.name), {
      type: "image/jpeg",
      lastModified: Date.now(),
    });
  }

  async function compressFormImages(form) {
    const fileInputs = form.querySelectorAll('input[type="file"][name="images"]');

    for (const input of fileInputs) {
      if (!input.files || input.files.length === 0 || !window.DataTransfer) {
        continue;
      }

      const transfer = new DataTransfer();
      for (const file of input.files) {
        transfer.items.add(await compressImage(file));
      }
      input.files = transfer.files;
    }
  }

  document.addEventListener("submit", async (event) => {
    const form = event.target;
    if (!isForumUploadForm(form) || form.dataset.forumImagesCompressed === "true") {
      if (isForumUploadForm(form)) {
        markSubmitting(form);
      }
      return;
    }
    if (!hasSelectedImages(form)) {
      markSubmitting(form);
      return;
    }

    event.preventDefault();
    if (processingForms.has(form)) {
      return;
    }
    processingForms.add(form);

    try {
      await compressFormImages(form);
    } catch (error) {
      console.warn("Forum image compression skipped:", error);
    }

    form.dataset.forumImagesCompressed = "true";
    markSubmitting(form);
    form.submit();
  });

  window.addEventListener("pageshow", () => {
    document.querySelectorAll("#newThreadForm").forEach(resetForumForm);
  });

  document.addEventListener("show.bs.modal", (event) => {
    resetForumForm(event.target.querySelector("#newThreadForm"));
  });

  document.addEventListener("hidden.bs.modal", (event) => {
    resetForumForm(event.target.querySelector("#newThreadForm"));
  });
})();
