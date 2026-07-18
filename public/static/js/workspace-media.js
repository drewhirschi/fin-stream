(function (global) {
    'use strict';

    const MAX_UPLOAD_BYTES = 25 * 1024 * 1024;
    const CONTENT_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp']);

    function bind(root) {
        root.querySelectorAll('[data-workspace-photo-upload]').forEach((form) => {
            if (form.dataset.workspacePhotoBound === 'true') return;
            form.dataset.workspacePhotoBound = 'true';
            form.addEventListener('submit', (event) => upload(event, form));
        });
    }

    function setStatus(form, message, isError) {
        const status = form.querySelector('[data-workspace-photo-status]');
        if (!status) return;
        status.textContent = message;
        status.classList.remove('hidden', 'text-error', 'text-success');
        status.classList.add(isError ? 'text-error' : 'text-success');
    }

    async function upload(event, form) {
        event.preventDefault();
        const input = form.querySelector('[data-workspace-photo-file]');
        const submit = form.querySelector('[data-workspace-photo-submit]');
        const file = input?.files?.[0];
        if (!file) {
            setStatus(form, 'Choose a photo first.', true);
            return;
        }
        if (!CONTENT_TYPES.has(file.type) || file.size < 1 || file.size > MAX_UPLOAD_BYTES) {
            setStatus(form, 'Choose a JPG, PNG, or WebP file no larger than 25 MB.', true);
            return;
        }
        if (!form.dataset.intentUrl || !form.dataset.finalizeUrl) {
            setStatus(form, 'Photo upload is unavailable.', true);
            return;
        }

        if (submit) submit.disabled = true;
        setStatus(form, 'Preparing direct upload…', false);
        try {
            const intent = await global.TrustDeedsUI.requestJson(
                form.dataset.intentUrl,
                {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        file_name: file.name,
                        content_type: file.type,
                        size_bytes: file.size,
                    }),
                },
                'Could not prepare the photo upload.'
            );
            if (intent?.upload?.method !== 'PUT'
                || typeof intent.upload.url !== 'string'
                || typeof intent.token !== 'string') {
                throw new Error('Could not prepare the photo upload. The server returned incomplete data.');
            }

            setStatus(form, 'Uploading directly to private storage…', false);
            let uploaded;
            try {
                uploaded = await fetch(intent.upload.url, {
                    method: 'PUT',
                    headers: intent.upload.headers ?? {},
                    body: file,
                    redirect: 'error',
                });
            } catch (_error) {
                throw new Error('The direct upload failed. Check storage CORS and try again.');
            }
            if (!uploaded.ok) {
                throw new Error('The direct upload was rejected. Choose the file and try again.');
            }

            setStatus(form, 'Verifying and saving the photo…', false);
            await global.TrustDeedsUI.requestJson(
                form.dataset.finalizeUrl,
                {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ token: intent.token }),
                },
                'The photo uploaded, but could not be saved yet. Try again.'
            );
            global.location.hash = 'workspace';
            global.location.reload();
        } catch (error) {
            setStatus(form, error?.message ?? 'Photo upload failed. Try again.', true);
            if (submit) submit.disabled = false;
        }
    }

    document.addEventListener('DOMContentLoaded', () => bind(document));
    document.body.addEventListener('htmx:load', (event) => bind(event.target));
})(window);
