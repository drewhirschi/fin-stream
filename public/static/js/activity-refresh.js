(() => {
    'use strict';

    const CHECK_INTERVAL_MS = 5 * 60 * 1000;
    const LAST_CHECK_KEY = 'trust_deeds_activity_refresh_checked_at';
    let inFlight = false;

    function localDate() {
        const now = new Date();
        return [
            now.getFullYear(),
            String(now.getMonth() + 1).padStart(2, '0'),
            String(now.getDate()).padStart(2, '0'),
        ].join('-');
    }

    async function refreshIfStale() {
        if (inFlight || document.visibilityState === 'hidden') return;

        const now = Date.now();
        const lastCheck = Number(sessionStorage.getItem(LAST_CHECK_KEY) || '0');
        if (Number.isFinite(lastCheck) && now - lastCheck < CHECK_INTERVAL_MS) return;

        inFlight = true;
        sessionStorage.setItem(LAST_CHECK_KEY, String(now));
        try {
            await fetch('/api/integrations/refresh-if-stale', {
                method: 'POST',
                credentials: 'same-origin',
                headers: {
                    'Accept': 'application/json',
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ as_of_date: localDate() }),
            });
        } catch (_) {
            // The durable execution log and integration status own failure
            // visibility. Page navigation should never fail because a
            // best-effort freshness check could not reach the server.
        } finally {
            inFlight = false;
        }
    }

    document.addEventListener('DOMContentLoaded', refreshIfStale);
    document.body.addEventListener('htmx:load', refreshIfStale);
    document.addEventListener('visibilitychange', refreshIfStale);
})();
