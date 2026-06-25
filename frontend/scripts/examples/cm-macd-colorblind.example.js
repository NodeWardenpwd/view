// 【示例脚本 — 勿引入程序】复制以下全部内容到「指标/策略编辑器」，保存 → 应用到图表即可。
// 无需修改 index.html 或任何程序代码。框架已内置 context.getCache() / closes() 等 API。
//
return {
    id: 'CM_Ult_MacD_Colorblind@tv-basicstudies-1',
    name: 'CM四色MACD+背离符号提示（对色盲友好版）',
    description: 'CM四色MACD+背离符号提示（对色盲友好版）',
    shortDescription: 'CM四色MACD',
    isPriceStudy: false,
    linkedToSeries: false,

    palettes: {
        histPalette: {
            colors: [
                { color: '#00C853', style: 0, width: 5 },
                { color: '#2E7D32', style: 0, width: 5 },
                { color: '#FF5252', style: 0, width: 5 },
                { color: '#BF360C', style: 0, width: 5 },
            ],
            valToIndex: { 0: 0, 1: 1, 2: 2, 3: 3 },
            addDefaultColor: true,
        },
        macdPalette: {
            colors: [
                { color: '#26A69A', style: 0, width: 2 },
                { color: '#FF5252', style: 0, width: 2 },
            ],
            valToIndex: { 0: 0, 1: 1 },
            addDefaultColor: true,
        },
    },

    inputs: [
        { id: 'fastLen', name: '快线周期', type: 'integer', defval: 12, min: 1, max: 200 },
        { id: 'slowLen', name: '慢线周期', type: 'integer', defval: 26, min: 1, max: 500 },
        { id: 'signalLen', name: '信号平滑', type: 'integer', defval: 9, min: 1, max: 200 },
        { id: 'divLookbackMax', name: '背离最大回溯', type: 'integer', defval: 60, min: 10, max: 200 },
        { id: 'divLookbackMin', name: '背离最小回溯', type: 'integer', defval: 5, min: 2, max: 30 },
        { id: 'pivotLen', name: 'Pivot左右宽度', type: 'integer', defval: 5, min: 2, max: 15 },
        { id: 'showMacdDiv', name: 'MACD背离', type: 'bool', defval: true },
        { id: 'showHistDiv', name: '柱体背离', type: 'bool', defval: true },
        { id: 'showTopDiv', name: '顶背离', type: 'bool', defval: true },
        { id: 'showBottomDiv', name: '底背离', type: 'bool', defval: true },
        { id: 'showHiddenTopDiv', name: '隐藏顶背离', type: 'bool', defval: true },
        { id: 'showHiddenBottomDiv', name: '隐藏底背离', type: 'bool', defval: true },
    ],

    plots: [
        { id: 'macd', type: 'line' },
        { id: 'macd_color', type: 'colorer', target: 'macd', palette: 'macdPalette' },
        { id: 'signal', type: 'line' },
        { id: 'hist', type: 'histogram' },
        { id: 'hist_color', type: 'colorer', target: 'hist', palette: 'histPalette' },
        { id: 'shrink_top_above0', type: 'shapes', shape: 'shape_arrow_down', location: 'AboveBar', color: '#2E7D32' },
        { id: 'shrink_foot_above0', type: 'shapes', shape: 'shape_arrow_up', location: 'BelowBar', color: '#26A69A' },
        { id: 'shrink_top_below0', type: 'shapes', shape: 'shape_triangle_down', location: 'AboveBar', color: '#FF5252' },
        { id: 'shrink_foot_below0', type: 'shapes', shape: 'shape_triangle_up', location: 'BelowBar', color: '#BF360C' },
        { id: 'macd_cross_above0', type: 'shapes', shape: 'shape_circle', location: 'BelowBar', color: '#26A69A' },
        { id: 'macd_cross_below0', type: 'shapes', shape: 'shape_circle', location: 'AboveBar', color: '#FF5252' },
        { id: 'hist_cross_above0', type: 'shapes', shape: 'shape_circle', location: 'BelowBar', color: '#00C853' },
        { id: 'hist_cross_below0', type: 'shapes', shape: 'shape_circle', location: 'AboveBar', color: '#BF360C' },
        { id: 'top_div', type: 'shapes', shape: 'shape_arrow_down', location: 'AboveBar', color: '#FF5252' },
        { id: 'bottom_div', type: 'shapes', shape: 'shape_arrow_up', location: 'BelowBar', color: '#26A69A' },
        { id: 'hist_bottom_div', type: 'shapes', shape: 'shape_triangle_up', location: 'BelowBar', color: '#00C853' },
    ],

    defaults: {
        styles: {
            macd: { linestyle: 0, linewidth: 2, plottype: 0, color: '#26A69A' },
            signal: { linestyle: 0, linewidth: 2, plottype: 0, color: '#FFD600' },
            hist: { linestyle: 0, linewidth: 5, plottype: 5, color: '#00C853' },
            shrink_top_above0: { plottype: 'shape_arrow_down', location: 'AboveBar', color: '#2E7D32', size: 'small' },
            shrink_foot_above0: { plottype: 'shape_arrow_up', location: 'BelowBar', color: '#26A69A', size: 'small' },
            shrink_top_below0: { plottype: 'shape_triangle_down', location: 'AboveBar', color: '#FF5252', size: 'small' },
            shrink_foot_below0: { plottype: 'shape_triangle_up', location: 'BelowBar', color: '#BF360C', size: 'small' },
            macd_cross_above0: { plottype: 'shape_circle', location: 'BelowBar', color: '#26A69A', size: 'tiny' },
            macd_cross_below0: { plottype: 'shape_circle', location: 'AboveBar', color: '#FF5252', size: 'tiny' },
            hist_cross_above0: { plottype: 'shape_circle', location: 'BelowBar', color: '#00C853', size: 'tiny' },
            hist_cross_below0: { plottype: 'shape_circle', location: 'AboveBar', color: '#BF360C', size: 'tiny' },
            top_div: { plottype: 'shape_arrow_down', location: 'AboveBar', color: '#FF5252', size: 'normal' },
            bottom_div: { plottype: 'shape_arrow_up', location: 'BelowBar', color: '#26A69A', size: 'normal' },
            hist_bottom_div: { plottype: 'shape_triangle_up', location: 'BelowBar', color: '#00C853', size: 'normal' },
        },
        palettes: {
            histPalette: {
                colors: [
                    { color: '#00C853', width: 5, style: 0 },
                    { color: '#2E7D32', width: 5, style: 0 },
                    { color: '#FF5252', width: 5, style: 0 },
                    { color: '#BF360C', width: 5, style: 0 },
                ],
            },
            macdPalette: {
                colors: [
                    { color: '#26A69A', width: 2, style: 0 },
                    { color: '#FF5252', width: 2, style: 0 },
                ],
            },
        },
    },

    styles: {
        macd: { title: 'MACD快线', type: 'line' },
        signal: { title: 'Signal慢线', type: 'line' },
        hist: { title: '四色动能柱', type: 'histogram' },
        shrink_top_above0: { title: '0轴上缩头', type: 'shapes' },
        shrink_foot_above0: { title: '0轴上收脚', type: 'shapes' },
        shrink_top_below0: { title: '0轴下缩头', type: 'shapes' },
        shrink_foot_below0: { title: '0轴下收脚', type: 'shapes' },
        macd_cross_above0: { title: 'MACD上穿0', type: 'shapes' },
        macd_cross_below0: { title: 'MACD下穿0', type: 'shapes' },
        hist_cross_above0: { title: '柱上穿0', type: 'shapes' },
        hist_cross_below0: { title: '柱下穿0', type: 'shapes' },
        top_div: { title: '顶背离', type: 'shapes' },
        bottom_div: { title: '底背离', type: 'shapes' },
        hist_bottom_div: { title: '柱底背', type: 'shapes' },
    },

    main(context, inputs) {
        const fastLen = Number(inputs.fastLen) || 12;
        const slowLen = Number(inputs.slowLen) || 26;
        const signalLen = Number(inputs.signalLen) || 9;
        const divMax = Number(inputs.divLookbackMax) || 60;
        const divMin = Number(inputs.divLookbackMin) || 5;
        const pivotLen = Number(inputs.pivotLen) || 5;
        const showMacdDiv = inputs.showMacdDiv !== false;
        const showHistDiv = inputs.showHistDiv !== false;
        const showTopDiv = inputs.showTopDiv !== false;
        const showBottomDiv = inputs.showBottomDiv !== false;
        const showHiddenTopDiv = inputs.showHiddenTopDiv !== false;
        const showHiddenBottomDiv = inputs.showHiddenBottomDiv !== false;

        if (context.is_first_bar || !context.getCache().fast) {
            context.resetCache();
            Object.assign(context.getCache(), {
                fast: [], slow: [], macd: [], signal: [], hist: [],
                closes: [], highs: [], lows: [],
                pivotHighs: [], pivotLows: [],
            });
        }
        const cache = context.getCache();

        const closePrice = typeof context.close === 'number' ? context.close : NaN;
        if (Number.isNaN(closePrice)) {
            return new Array(16).fill(NaN);
        }

        const fastAlpha = 2 / (fastLen + 1);
        const slowAlpha = 2 / (slowLen + 1);
        let currentFast = closePrice;
        let currentSlow = closePrice;
        if (cache.fast.length > 0) {
            const pf = cache.fast[cache.fast.length - 1];
            const ps = cache.slow[cache.slow.length - 1];
            currentFast = closePrice * fastAlpha + pf * (1 - fastAlpha);
            currentSlow = closePrice * slowAlpha + ps * (1 - slowAlpha);
        }
        cache.fast.push(currentFast);
        cache.slow.push(currentSlow);

        const macdVal = currentFast - currentSlow;
        cache.macd.push(macdVal);

        let signalSum = 0;
        let signalCount = 0;
        for (let i = 0; i < signalLen; i++) {
            const li = cache.macd.length - 1 - i;
            if (li >= 0 && !Number.isNaN(cache.macd[li])) {
                signalSum += cache.macd[li];
                signalCount++;
            }
        }
        const signalVal = signalCount > 0 ? signalSum / signalCount : macdVal;
        cache.signal.push(signalVal);

        const histVal = macdVal - signalVal;
        const prevHistVal = cache.hist.length > 0 ? cache.hist[cache.hist.length - 1] : 0;
        const prevMacdVal = cache.macd.length > 1 ? cache.macd[cache.macd.length - 2] : 0;
        cache.hist.push(histVal);
        cache.closes.push(closePrice);
        cache.highs.push(typeof context.high === 'number' ? context.high : closePrice);
        cache.lows.push(typeof context.low === 'number' ? context.low : closePrice);

        const idx = cache.hist.length - 1;

        if (idx + 1 < slowLen) {
            return new Array(16).fill(NaN);
        }

        let histColorIdx = 0;
        if (histVal > 0) {
            histColorIdx = histVal > prevHistVal ? 0 : 1;
        } else if (histVal < 0) {
            histColorIdx = histVal < prevHistVal ? 2 : 3;
        }

        const macdColorIdx = macdVal >= signalVal ? 0 : 1;

        const crossUp = (prev, cur) => prev <= 0 && cur > 0;
        const crossDown = (prev, cur) => prev >= 0 && cur < 0;

        const shrinkTopAbove = histVal > 0 && histVal < prevHistVal;
        const shrinkFootAbove = histVal > 0 && histVal > prevHistVal && prevHistVal <= (cache.hist.length > 2 ? cache.hist[cache.hist.length - 2] : prevHistVal);
        const shrinkTopBelow = histVal < 0 && histVal < prevHistVal;
        const shrinkFootBelow = histVal < 0 && histVal > prevHistVal;

        const macdCrossAbove = crossUp(prevMacdVal, macdVal);
        const macdCrossBelow = crossDown(prevMacdVal, macdVal);
        const histCrossAbove = crossUp(prevHistVal, histVal);
        const histCrossBelow = crossDown(prevHistVal, histVal);

        function isPivotHigh(i, len) {
            if (i < len || i >= cache.closes.length - len) return false;
            const p = cache.closes[i];
            for (let j = i - len; j <= i + len; j++) {
                if (j !== i && cache.closes[j] >= p) return false;
            }
            return true;
        }

        function isPivotLow(i, len) {
            if (i < len || i >= cache.closes.length - len) return false;
            const p = cache.closes[i];
            for (let j = i - len; j <= i + len; j++) {
                if (j !== i && cache.closes[j] <= p) return false;
            }
            return true;
        }

        let topDiv = false;
        let bottomDiv = false;
        let histBottomDiv = false;
        let hiddenTopDiv = false;
        let hiddenBottomDiv = false;

        if (idx >= pivotLen * 2 && isPivotHigh(idx - pivotLen, pivotLen)) {
            const pi = idx - pivotLen;
            const pivot = { idx: pi, price: cache.closes[pi], macd: cache.macd[pi], hist: cache.hist[pi] };
            cache.pivotHighs.push(pivot);
            if (cache.pivotHighs.length > 20) cache.pivotHighs.shift();

            if (cache.pivotHighs.length >= 2) {
                const prev = cache.pivotHighs[cache.pivotHighs.length - 2];
                const cur = cache.pivotHighs[cache.pivotHighs.length - 1];
                const dist = cur.idx - prev.idx;
                if (dist >= divMin && dist <= divMax) {
                    if (showTopDiv && cur.price > prev.price && cur.macd < prev.macd) topDiv = true;
                    if (showHiddenTopDiv && cur.price < prev.price && cur.macd > prev.macd) hiddenTopDiv = true;
                    if (showHistDiv && showTopDiv && cur.price > prev.price && cur.hist < prev.hist) topDiv = true;
                }
            }
        }

        if (idx >= pivotLen * 2 && isPivotLow(idx - pivotLen, pivotLen)) {
            const pi = idx - pivotLen;
            const pivot = { idx: pi, price: cache.closes[pi], macd: cache.macd[pi], hist: cache.hist[pi] };
            cache.pivotLows.push(pivot);
            if (cache.pivotLows.length > 20) cache.pivotLows.shift();

            if (cache.pivotLows.length >= 2) {
                const prev = cache.pivotLows[cache.pivotLows.length - 2];
                const cur = cache.pivotLows[cache.pivotLows.length - 1];
                const dist = cur.idx - prev.idx;
                if (dist >= divMin && dist <= divMax) {
                    if (showBottomDiv && cur.price < prev.price && cur.macd > prev.macd) bottomDiv = true;
                    if (showHiddenBottomDiv && cur.price > prev.price && cur.macd < prev.macd) hiddenBottomDiv = true;
                    if (showHistDiv && showBottomDiv && cur.price < prev.price && cur.hist > prev.hist) {
                        histBottomDiv = true;
                        bottomDiv = true;
                    }
                }
            }
        }

        if (showMacdDiv === false) { topDiv = false; bottomDiv = false; hiddenTopDiv = false; hiddenBottomDiv = false; }

        const shapeYTop = macdVal + Math.abs(histVal) * 0.35 + 0.01;
        const shapeYBottom = macdVal - Math.abs(histVal) * 0.35 - 0.01;

        const shapeNaN = NaN;

        return [
            macdVal,
            macdColorIdx,
            signalVal,
            histVal,
            histColorIdx,
            shrinkTopAbove ? shapeYTop : shapeNaN,
            shrinkFootAbove ? shapeYBottom : shapeNaN,
            shrinkTopBelow ? shapeYTop : shapeNaN,
            shrinkFootBelow ? shapeYBottom : shapeNaN,
            macdCrossAbove ? shapeYBottom : shapeNaN,
            macdCrossBelow ? shapeYTop : shapeNaN,
            histCrossAbove ? shapeYBottom : shapeNaN,
            histCrossBelow ? shapeYTop : shapeNaN,
            (topDiv || hiddenTopDiv) ? shapeYTop : shapeNaN,
            (bottomDiv || hiddenBottomDiv) ? shapeYBottom : shapeNaN,
            histBottomDiv ? shapeYBottom : shapeNaN,
        ];
    },
};
