(function (global) {
    'use strict';

    function numeric(value) {
        const parsed = Number(value);
        return Number.isFinite(parsed) ? parsed : 0;
    }

    function parseMoney(value) {
        const normalized = String(value ?? '')
            .trim()
            .replaceAll(',', '')
            .replace('$', '');
        return normalized === '' ? Number.NaN : Number(normalized);
    }

    function money(value) {
        const amount = numeric(value);
        const absolute = Math.abs(amount);
        if (absolute < 0.005) return '0';

        const formatted = absolute.toLocaleString('en-US', {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
        });
        return amount < 0 ? `-${formatted}` : formatted;
    }

    function currency(value) {
        const amount = numeric(value);
        const formatted = money(amount);
        if (formatted === '0') return '0';
        return amount < 0 ? `-$${formatted.slice(1)}` : `$${formatted}`;
    }

    function date(value) {
        const raw = String(value ?? '').trim();
        const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(raw);
        return match ? `${match[2]}-${match[3]}-${match[1]}` : raw;
    }

    function datetime(value) {
        const raw = String(value ?? '').trim();
        if (!raw) return '';

        const parsed = new Date(raw);
        if (Number.isNaN(parsed.getTime())) return raw;

        const parts = new Intl.DateTimeFormat('en-US', {
            month: '2-digit',
            day: '2-digit',
            year: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
            hour12: true,
        }).formatToParts(parsed);
        const part = (type) => parts.find((item) => item.type === type)?.value ?? '';
        return `${part('month')}-${part('day')}-${part('year')} ${part('hour')}:${part('minute')} ${part('dayPeriod')}`;
    }

    async function responseJson(response, fallbackMessage) {
        const contentType = response.headers.get('content-type') ?? '';
        let payload = null;

        if (contentType.includes('application/json')) {
            payload = await response.json().catch(() => null);
        }

        if (!response.ok || payload?.error) {
            const message = payload?.message
                ?? (response.status === 401 ? 'Your session expired. Sign in and try again.' : null)
                ?? fallbackMessage;
            throw new Error(message);
        }

        if (payload === null) {
            throw new Error(`${fallbackMessage} The server returned an unexpected response.`);
        }

        return payload;
    }

    async function requestJson(url, options, fallbackMessage) {
        let response;
        try {
            response = await fetch(url, {
                ...options,
                headers: {
                    Accept: 'application/json',
                    ...(options?.headers ?? {}),
                },
            });
        } catch (_error) {
            throw new Error(`${fallbackMessage} Check your connection and try again.`);
        }
        return responseJson(response, fallbackMessage);
    }

    global.TrustDeedsUI = Object.freeze({
        money,
        currency,
        parseMoney,
        date,
        datetime,
        requestJson,
        responseJson,
    });
})(window);
