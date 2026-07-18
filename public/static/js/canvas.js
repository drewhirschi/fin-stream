(function (global) {
    'use strict';

    const DAY_MS = 86400000;
    const BASE_DAY_WIDTH = 12;
    const RANGE_PAST_DAYS = 45;
    const RANGE_FUTURE_DAYS = 365;
    const COLORS = ['#4edea3', '#60a5fa', '#f59e0b', '#ef4444', '#a78bfa', '#22d3ee'];

    function isoDate(value) {
        return new Date(value).toISOString().split('T')[0];
    }

    function localIsoDate(value) {
        return [
            value.getFullYear(),
            String(value.getMonth() + 1).padStart(2, '0'),
            String(value.getDate()).padStart(2, '0'),
        ].join('-');
    }

    function addDays(dateString, days) {
        const date = new Date(`${dateString}T00:00:00Z`);
        date.setUTCDate(date.getUTCDate() + days);
        return isoDate(date);
    }

    function dayIndex(dateString) {
        return Math.floor(new Date(`${dateString}T00:00:00Z`).getTime() / DAY_MS);
    }

    global.infiniteCanvas = function infiniteCanvas() {
        return {
            canvas: null,
            ctx: null,
            viewport: null,
            width: 0,
            height: 0,
            devicePixelRatio: global.devicePixelRatio || 1,
            offsetX: 0,
            offsetY: 0,
            scale: 1,
            minScale: 0.2,
            maxScale: 3.5,
            dragging: false,
            lastPointer: null,
            showImportMenu: false,
            importedStreams: [],
            hoverInfo: null,
            today: localIsoDate(new Date()),
            visibleRangeStart: null,
            visibleRangeEnd: null,

            get zoomLabel() {
                return `${Math.round(this.scale * 100)}%`;
            },

            get worldOrigin() {
                return {
                    x: (-this.offsetX) / this.scale,
                    y: (-this.offsetY) / this.scale,
                };
            },

            get showDailyTicks() {
                return this.scale >= 1;
            },

            init() {
                this.visibleRangeStart = addDays(this.today, -RANGE_PAST_DAYS);
                this.visibleRangeEnd = addDays(this.today, RANGE_FUTURE_DAYS);
                this.viewport = this.$refs.viewport;
                this.canvas = this.$refs.canvas;
                this.ctx = this.canvas.getContext('2d');
                this.resetView();
                this.resize();

                const defaultStreamId = Number(this.$root.dataset.defaultStreamId || 0);
                if (defaultStreamId > 0) {
                    const defaultButton = this.$root.querySelector(`[data-stream-id="${defaultStreamId}"]`);
                    const defaultName = defaultButton ? defaultButton.dataset.streamName : 'Trust Deeds';
                    this.importStream({ id: defaultStreamId, name: defaultName });
                }
                global.addEventListener('resize', () => this.resize());
            },

            resize() {
                const rect = this.viewport.getBoundingClientRect();
                this.width = rect.width;
                this.height = rect.height;
                this.canvas.width = Math.floor(rect.width * this.devicePixelRatio);
                this.canvas.height = Math.floor(rect.height * this.devicePixelRatio);
                this.ctx.setTransform(1, 0, 0, 1, 0, 0);
                this.ctx.scale(this.devicePixelRatio, this.devicePixelRatio);
                this.draw();
            },

            resetView() {
                if (this.viewport) {
                    const rect = this.viewport.getBoundingClientRect();
                    this.offsetX = rect.width * 0.18;
                    this.offsetY = rect.height * 0.22;
                } else {
                    this.offsetX = 160;
                    this.offsetY = 120;
                }
                this.scale = 1;
                this.draw();
            },

            async importStream(stream) {
                if (this.importedStreams.some((item) => item.id === stream.id)) {
                    this.showImportMenu = false;
                    return;
                }

                const color = COLORS[this.importedStreams.length % COLORS.length];
                const laneTop = 120 + this.importedStreams.length * 150;
                const events = await this.fetchStreamEvents(stream.id);
                const eventsByDay = new Map();
                for (const event of events) {
                    const day = dayIndex(event.date);
                    const bucket = eventsByDay.get(day) || [];
                    bucket.push({
                        ...event,
                        amountLabel: global.TrustDeedsUI.currency(Number(event.amount || 0)),
                    });
                    eventsByDay.set(day, bucket);
                }
                const startDay = dayIndex(this.visibleRangeStart);
                const endDay = dayIndex(this.visibleRangeEnd);

                this.importedStreams.push({
                    id: stream.id,
                    name: stream.name,
                    color,
                    x: 0,
                    y: laneTop,
                    width: Math.max(endDay - startDay, 1) * BASE_DAY_WIDTH,
                    height: 72,
                    startDay,
                    endDay,
                    eventsByDay,
                });

                this.showImportMenu = false;
                this.draw();
            },

            importFromButton(button) {
                this.importStream({
                    id: Number(button.dataset.streamId),
                    name: button.dataset.streamName,
                });
            },

            async fetchStreamEvents(streamId) {
                try {
                    const data = await global.TrustDeedsUI.requestJson(
                        `/api/forecast?from=${this.visibleRangeStart}&through=${this.visibleRangeEnd}&stream_id=${streamId}`,
                        {},
                        'Canvas events are unavailable.'
                    );
                    return Array.isArray(data.rows)
                        ? data.rows.filter((row) => row.expected_date || row.status !== 'received')
                        : [];
                } catch (_error) {
                    return [];
                }
            },

            pointerDown(event) {
                this.dragging = true;
                this.lastPointer = { x: event.clientX, y: event.clientY };
                this.viewport.setPointerCapture(event.pointerId);
                this.viewport.style.cursor = 'grabbing';
            },

            pointerMove(event) {
                if (this.dragging && this.lastPointer) {
                    const dx = event.clientX - this.lastPointer.x;
                    const dy = event.clientY - this.lastPointer.y;
                    this.offsetX += dx;
                    this.offsetY += dy;
                    this.lastPointer = { x: event.clientX, y: event.clientY };
                    this.hoverInfo = null;
                    this.draw();
                    return;
                }
                this.updateHover(event);
            },

            pointerUp(event) {
                if (this.dragging) {
                    this.dragging = false;
                    this.lastPointer = null;
                    if (this.viewport.hasPointerCapture(event.pointerId)) {
                        this.viewport.releasePointerCapture(event.pointerId);
                    }
                    this.viewport.style.cursor = 'grab';
                }
            },

            wheelZoom(event) {
                const rect = this.viewport.getBoundingClientRect();
                const pointerX = event.clientX - rect.left;
                const pointerY = event.clientY - rect.top;
                const worldX = (pointerX - this.offsetX) / this.scale;
                const worldY = (pointerY - this.offsetY) / this.scale;
                const zoomFactor = event.deltaY < 0 ? 1.08 : 0.92;
                const nextScale = Math.min(this.maxScale, Math.max(this.minScale, this.scale * zoomFactor));
                if (nextScale === this.scale) return;

                this.scale = nextScale;
                this.offsetX = pointerX - worldX * this.scale;
                this.offsetY = pointerY - worldY * this.scale;
                this.draw();
                this.updateHover(event);
            },

            worldToScreen(point) {
                return {
                    x: this.offsetX + point.x * this.scale,
                    y: this.offsetY + point.y * this.scale,
                };
            },

            screenToWorld(point) {
                return {
                    x: (point.x - this.offsetX) / this.scale,
                    y: (point.y - this.offsetY) / this.scale,
                };
            },

            draw() {
                if (!this.ctx) return;
                this.ctx.clearRect(0, 0, this.width, this.height);
                this.drawGrid();
                this.drawStreamLines();
                this.drawAxes();
            },

            drawGrid() {
                const base = 48;
                const fineSpacing = base * this.scale;
                const heavySpacing = base * 4 * this.scale;

                this.ctx.save();
                this.ctx.strokeStyle = 'rgba(134, 148, 138, 0.10)';
                this.ctx.lineWidth = 1;
                this.drawGridLines(fineSpacing);
                this.ctx.strokeStyle = 'rgba(134, 148, 138, 0.22)';
                this.ctx.lineWidth = 1.1;
                this.drawGridLines(heavySpacing);
                this.ctx.restore();
            },

            drawGridLines(spacing) {
                if (spacing < 14) return;
                const startX = ((this.offsetX % spacing) + spacing) % spacing;
                const startY = ((this.offsetY % spacing) + spacing) % spacing;

                this.ctx.beginPath();
                for (let x = startX; x <= this.width; x += spacing) {
                    this.ctx.moveTo(x, 0);
                    this.ctx.lineTo(x, this.height);
                }
                for (let y = startY; y <= this.height; y += spacing) {
                    this.ctx.moveTo(0, y);
                    this.ctx.lineTo(this.width, y);
                }
                this.ctx.stroke();
            },

            drawStreamLines() {
                for (const stream of this.importedStreams) {
                    this.drawStreamCard(stream);
                }
            },

            drawStreamCard(stream) {
                const baselineY = stream.y + stream.height / 2;
                const cardOrigin = this.worldToScreen({ x: stream.x, y: stream.y });
                const cardBottomRight = this.worldToScreen({ x: stream.x + stream.width, y: stream.y + stream.height });
                const width = cardBottomRight.x - cardOrigin.x;
                const height = cardBottomRight.y - cardOrigin.y;

                if (cardBottomRight.x < -120 || cardOrigin.x > this.width + 120 || cardBottomRight.y < -120 || cardOrigin.y > this.height + 120) {
                    return;
                }

                this.ctx.save();
                this.ctx.fillStyle = 'rgba(11, 13, 15, 0.58)';
                this.ctx.strokeStyle = 'rgba(134, 148, 138, 0.18)';
                this.ctx.lineWidth = 1;
                this.roundRect(cardOrigin.x - 12, cardOrigin.y - 16, width + 24, height + 32, 18);
                this.ctx.fill();
                this.ctx.stroke();

                this.ctx.fillStyle = '#e1e2e7';
                this.ctx.font = '600 14px Inter';
                this.ctx.fillText(stream.name, cardOrigin.x, cardOrigin.y - 24);

                this.ctx.strokeStyle = 'rgba(134, 148, 138, 0.28)';
                this.ctx.lineWidth = 1;
                const baselineScreen = this.worldToScreen({ x: stream.x, y: baselineY });
                this.ctx.beginPath();
                this.ctx.moveTo(cardOrigin.x, baselineScreen.y);
                this.ctx.lineTo(cardBottomRight.x, baselineScreen.y);
                this.ctx.stroke();

                if (this.showDailyTicks) {
                    this.ctx.strokeStyle = 'rgba(134, 148, 138, 0.22)';
                    this.ctx.beginPath();
                    for (let index = 0; index <= (stream.endDay - stream.startDay); index += 1) {
                        const tickPoint = this.worldToScreen({ x: stream.x + index * BASE_DAY_WIDTH, y: baselineY });
                        this.ctx.moveTo(tickPoint.x, baselineScreen.y - 8);
                        this.ctx.lineTo(tickPoint.x, baselineScreen.y + 8);
                    }
                    this.ctx.stroke();
                }

                if (this.hoverInfo && this.hoverInfo.streamId === stream.id) {
                    this.ctx.strokeStyle = 'rgba(225, 226, 231, 0.38)';
                    this.ctx.lineWidth = 1;
                    this.ctx.beginPath();
                    this.ctx.moveTo(this.hoverInfo.screenX, cardOrigin.y);
                    this.ctx.lineTo(this.hoverInfo.screenX, cardBottomRight.y);
                    this.ctx.stroke();

                    for (let index = 0; index < this.hoverInfo.events.length; index += 1) {
                        const event = this.hoverInfo.events[index];
                        const verticalOffset = event.amount >= 0 ? -14 - index * 12 : 14 + index * 12;
                        this.ctx.fillStyle = event.amount >= 0 ? '#4edea3' : '#ef4444';
                        this.ctx.beginPath();
                        this.ctx.arc(this.hoverInfo.screenX, baselineScreen.y + verticalOffset, 5.5, 0, Math.PI * 2);
                        this.ctx.fill();
                    }
                }

                this.ctx.restore();
            },

            updateHover(event) {
                const rect = this.viewport.getBoundingClientRect();
                const pointer = {
                    x: event.clientX - rect.left,
                    y: event.clientY - rect.top,
                };

                let best = null;
                for (const stream of this.importedStreams) {
                    const baselineY = this.worldToScreen({ x: stream.x, y: stream.y + stream.height / 2 }).y;
                    const startX = this.worldToScreen({ x: stream.x, y: stream.y }).x;
                    const endX = this.worldToScreen({ x: stream.x + stream.width, y: stream.y }).x;
                    const withinX = pointer.x >= Math.min(startX, endX) && pointer.x <= Math.max(startX, endX);
                    const distanceY = Math.abs(pointer.y - baselineY);

                    if (!withinX || distanceY > 18) continue;

                    const worldPoint = this.screenToWorld(pointer);
                    const hoveredDay = Math.max(
                        stream.startDay,
                        Math.min(stream.endDay, stream.startDay + Math.round((worldPoint.x - stream.x) / BASE_DAY_WIDTH))
                    );
                    const hoverX = this.worldToScreen({
                        x: stream.x + (hoveredDay - stream.startDay) * BASE_DAY_WIDTH,
                        y: stream.y + stream.height / 2,
                    }).x;
                    const events = stream.eventsByDay.get(hoveredDay) || [];

                    if (!best || distanceY < best.distance) {
                        best = {
                            distance: distanceY,
                            streamId: stream.id,
                            streamName: stream.name,
                            date: global.TrustDeedsUI.date(isoDate(new Date(hoveredDay * DAY_MS))),
                            events,
                            screenX: hoverX,
                            screenY: baselineY,
                        };
                    }
                }

                this.hoverInfo = best || null;
                this.draw();
            },

            drawAxes() {
                const x = this.offsetX;
                const y = this.offsetY;

                this.ctx.save();
                this.ctx.strokeStyle = 'rgba(78, 222, 163, 0.5)';
                this.ctx.lineWidth = 1.5;
                this.ctx.beginPath();
                this.ctx.moveTo(x, 0);
                this.ctx.lineTo(x, this.height);
                this.ctx.moveTo(0, y);
                this.ctx.lineTo(this.width, y);
                this.ctx.stroke();

                this.ctx.fillStyle = '#4edea3';
                this.ctx.beginPath();
                this.ctx.arc(x, y, 5, 0, Math.PI * 2);
                this.ctx.fill();
                this.ctx.restore();
            },

            roundRect(x, y, width, height, radius) {
                this.ctx.beginPath();
                this.ctx.moveTo(x + radius, y);
                this.ctx.lineTo(x + width - radius, y);
                this.ctx.quadraticCurveTo(x + width, y, x + width, y + radius);
                this.ctx.lineTo(x + width, y + height - radius);
                this.ctx.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
                this.ctx.lineTo(x + radius, y + height);
                this.ctx.quadraticCurveTo(x, y + height, x, y + height - radius);
                this.ctx.lineTo(x, y + radius);
                this.ctx.quadraticCurveTo(x, y, x + radius, y);
                this.ctx.closePath();
            },
        };
    };
})(window);
