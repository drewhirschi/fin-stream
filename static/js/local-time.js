// Upgrade server-rendered <time data-local="..."> elements to the viewer's
// local timezone. The server emits RFC3339 (UTC) in `datetime=` and
// `data-local=` attributes plus a "... UTC" text fallback for the no-JS case.
//
// We swap the text to a localized "MM-DD-YYYY hh:mm AM/PM TZ" string and
// flag the element with `data-local-applied` so htmx re-swaps don't cause
// flicker or re-apply.
(function () {
    'use strict';

    var MONTHS = null; // not needed — we build format parts manually

    function pad(n) {
        n = Math.abs(n | 0);
        return n < 10 ? '0' + n : '' + n;
    }

    function buildParts(date) {
        // Ask Intl for the TZ abbreviation (e.g. "EDT"). Not every runtime
        // supports it, so fall back gracefully.
        var tzAbbr = '';
        try {
            var fmt = new Intl.DateTimeFormat(undefined, {
                timeZoneName: 'short'
            });
            var parts = fmt.formatToParts(date);
            for (var i = 0; i < parts.length; i++) {
                if (parts[i].type === 'timeZoneName') {
                    tzAbbr = parts[i].value;
                    break;
                }
            }
        } catch (e) {
            tzAbbr = '';
        }

        var month = pad(date.getMonth() + 1);
        var day = pad(date.getDate());
        var year = date.getFullYear();

        var hours24 = date.getHours();
        var ampm = hours24 >= 12 ? 'PM' : 'AM';
        var hours12 = hours24 % 12;
        if (hours12 === 0) hours12 = 12;
        var minutes = pad(date.getMinutes());

        var base = month + '-' + day + '-' + year + ' ' + pad(hours12) + ':' + minutes + ' ' + ampm;
        return tzAbbr ? base + ' ' + tzAbbr : base;
    }

    function upgrade(root) {
        if (!root) return;
        var nodes = root.querySelectorAll('time[data-local]:not([data-local-applied])');
        for (var i = 0; i < nodes.length; i++) {
            var el = nodes[i];
            var iso = el.getAttribute('data-local') || el.getAttribute('datetime');
            if (!iso) continue;
            var date = new Date(iso);
            if (isNaN(date.getTime())) continue;
            try {
                el.textContent = buildParts(date);
                el.setAttribute('data-local-applied', '1');
            } catch (e) {
                // leave server fallback in place
            }
        }
    }

    function onReady() {
        upgrade(document);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', onReady);
    } else {
        onReady();
    }

    // htmx swaps: upgrade any new <time> elements in the swapped region.
    document.body && document.body.addEventListener('htmx:afterSwap', function (evt) {
        upgrade(evt.target || document);
    });
    document.addEventListener('htmx:afterSwap', function (evt) {
        upgrade(evt.target || document);
    });
})();
