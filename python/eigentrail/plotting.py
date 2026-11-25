"""Display-only views of independently checked rolling lineage reports.

Every plotted decision, count, interval and path is read from checked records of a
``RollingResult``; nothing spectral or graph-theoretic is recomputed here.
Matplotlib is imported lazily and no TeX is used. Save figures with the caller's
``figure.savefig(path)``: ``.svg`` and ``.pdf`` are vector output and PNG should
use ``figure.savefig('out.png', dpi=300)``.
"""
import base64
import html
from functools import wraps
from pathlib import Path

from .rolling import RollingResult

_LENSES = ('covariance', 'precision', 'correlation', 'partial')
_STATUS = {'agreement': 'A', 'exclusion': 'E', 'open': '?', 'unavailable': 'NA'}
_STYLE = {'agreement': '-', 'exclusion': ':', 'open': '--', 'unavailable': '-.'}
_COLOUR = {'agreement': 'tab:green', 'exclusion': 'tab:red', 'open': 'tab:orange',
           'unavailable': 'tab:gray'}
_COUNT_KEYS = ('agreement', 'exclusion', 'open', 'unavailable')


def _pyplot():
    import matplotlib.pyplot as plt
    return plt


def _without_tex(function):
    @wraps(function)
    def draw(*args, **kwargs):
        plt = _pyplot()
        with plt.rc_context({'text.usetex': False, 'text.parse_math': False}):
            output = function(*args, **kwargs)
            output[0].canvas.draw()
            return output
    return draw


def _checked(result):
    if not isinstance(result, RollingResult):
        raise TypeError('a checked RollingResult is required')
    return result


def _node_index(result):
    """node id -> {subject,date,lens,rank:[lo,hi) zero-based} from checked partitions."""
    index = {}
    for part in result.records('partitions'):
        state = part['state']
        if state['kind'] != 'groups':
            continue
        for offset, rank in enumerate(state['ranks']):
            index[part['first_node'] + offset] = {
                'subject': part['subject'], 'date': part['date'],
                'lens': part['lens'], 'rank': list(rank)}
    return index


def _rank_label(rank):
    lo, hi = rank[0] + 1, rank[1]
    return str(lo) if lo == hi else '%d-%d' % (lo, hi)


def _node_label(index, node):
    info = index.get(node)
    if info is None:
        return 'node %d (unindexed)' % node
    return '%s r%s @d%d' % (info['lens'], _rank_label(info['rank']), info['date'])


def _enclosure(value):
    if value['kind'] == 'bounds':
        return 'approx [%.6g, %.6g] deg' % tuple(value['range'])
    return '%s (%s)' % (value['kind'], value['reason'])


def _date_labels(result):
    return [str(d['id']) + (' (t=%.6g)' % d['time'] if str(d['id']) != '%.6g' % d['time'] else '')
            for d in result.request['dates']]


def _mode_text(result):
    unc = result.request['uncertainty']
    text = 'mode=%s, tolerance=%.6g deg' % (unc['mode'], result.request['tolerance_deg'])
    if unc['mode'] == 'conditional':
        text += ', rho=%.6g, event=%s, provider=%s' % (unc['rho'], unc['joint_event'], unc['provider'])
    return text


def _projection_kind(result):
    return result.request['projection']['kind']


def _block_selected(result, subject, from_date, to_date, pair):
    projection = result.request['projection']
    if projection['kind'] == 'full':
        return True
    if projection['kind'] == 'counts':
        return False
    return any(b['subject'] == subject and b['from_date'] == from_date and
               b['to_date'] == to_date and b['source_lens'] == pair[0] and
               b['target_lens'] == pair[1] for b in projection['blocks'])


def _window(result, subject, from_date, to_date):
    for window in result.records('windows'):
        if (window['subject'], window['from_date'], window['to_date']) == (subject, from_date, to_date):
            return window
    raise ValueError('no checked WindowSummary for subject %d, dates %d->%d'
                     % (subject, from_date, to_date))


def _window_edges(result, index, subject, from_date, to_date):
    """Checked edge rows grouped by (source lens, target lens) for one window."""
    groups = {}
    for edge in result.records('edges'):
        src, dst = index.get(edge['source']), index.get(edge['target'])
        if src is None or dst is None:
            continue
        if (src['subject'], dst['subject'], src['date'], dst['date']) != (subject, subject, from_date, to_date):
            continue
        groups.setdefault((src['lens'], dst['lens']), []).append(edge)
    return groups


def _verify_counts(pair, counts, edges):
    observed = {key: sum(1 for e in edges if e['verdict'] == key) for key in _COUNT_KEYS}
    if counts['candidates'] != len(edges) or any(counts[k] != observed[k] for k in _COUNT_KEYS):
        raise ValueError('edge rows for %s->%s disagree with checked window counts' % tuple(pair))
    ids = [e['ordinal'] for e in edges]
    if len(set(ids)) != len(ids):
        raise ValueError('duplicate edge ordinals for %s->%s' % tuple(pair))


def _count_text(counts):
    return 'n=%d  A=%d E=%d ?=%d NA=%d' % (counts['candidates'], counts['agreement'],
        counts['exclusion'], counts['open'], counts['unavailable'])


def _draw_edges(ax, index, edges):
    ranks = set()
    for edge in edges:
        src, dst = index[edge['source']], index[edge['target']]
        y0, y1 = (src['rank'][0] + src['rank'][1] + 1) / 2, (dst['rank'][0] + dst['rank'][1] + 1) / 2
        verdict = edge['verdict']
        ax.plot([0, 1], [y0, y1], linestyle=_STYLE[verdict], color=_COLOUR[verdict], linewidth=1)
        angle = edge['method_bracket']
        label = _STATUS[verdict]
        if angle['kind'] == 'bounds':
            label += '\n≈[%.3g, %.3g]°' % tuple(angle['range'])
        ax.text(0.5, (y0 + y1) / 2, label, ha='center', va='center', fontsize=6,
                bbox={'boxstyle': 'round,pad=0.1', 'fc': 'white', 'ec': 'none'})
        ax.text(-0.04, y0, 'r' + _rank_label(src['rank']), ha='right', va='center', fontsize=6)
        ax.text(1.04, y1, 'r' + _rank_label(dst['rank']), ha='left', va='center', fontsize=6)
        ranks.update((y0, y1))
    if ranks:
        ax.set_ylim(max(ranks) + 0.8, min(ranks) - 0.8)
    ax.set_xlim(-0.35, 1.35)


@_without_tex
def plot_window(result, from_date, to_date, *, subject=0):
    """4x4 source(rows) x destination(columns) lens matrix for one checked window.

    Counts come from the checked WindowSummary.lens_pairs. Edge rows are drawn only
    when the projection retained them, in which case ordinals and counts are verified.
    Each Axes carries ``_eigentrail_edge_ids`` (tuple of ordinals, or None when rows
    are absent) and ``_eigentrail_counts`` (checked Counts dict or None).
    Returns ``(figure, axes)`` with ``axes`` a 4x4 array.
    """
    result = _checked(result)
    plt = _pyplot()
    window = _window(result, subject, from_date, to_date)
    index = _node_index(result)
    rows = _window_edges(result, index, subject, from_date, to_date)
    pairs = {tuple(lw['pair']): lw for lw in window['lens_pairs']}
    dates = _date_labels(result)
    fig, axes = plt.subplots(4, 4, figsize=(15, 13), squeeze=False)
    for i, a in enumerate(_LENSES):
        for j, b in enumerate(_LENSES):
            ax = axes[i, j]
            ax.set_xticks([])
            ax.set_yticks([])
            ax._eigentrail_edge_ids = None
            ax._eigentrail_counts = None
            ax._eigentrail_header = None
            if i == 0:
                ax.set_title('destination:\n' + b, fontsize=8)
            if j == 0:
                ax.set_ylabel(a, fontsize=8)
            lw = pairs.get((a, b))
            if lw is None:
                ax.text(0.5, 0.5, 'not in lens scope', ha='center', va='center',
                        transform=ax.transAxes, fontsize=8, style='italic')
                ax.set_facecolor('#f2f2f2')
                continue
            counts = lw['counts']
            ax._eigentrail_counts = dict(counts)
            outcome = {'agreement': 'any A', 'complete_break': 'all E',
                       'open': 'open', 'unavailable': 'unavailable'}[lw['outcome']]
            header = 'n=%d A=%d E=%d\n?=%d NA=%d · %s' % (counts['candidates'],
                counts['agreement'], counts['exclusion'], counts['open'], counts['unavailable'], outcome)
            ax._eigentrail_header = ax.text(0.5, 0.98, header, ha='center', va='top',
                fontsize=6.5, linespacing=1.1, transform=ax.transAxes)
            if _block_selected(result, subject, from_date, to_date, (a, b)):
                edges = sorted(rows.get((a, b), []), key=lambda e: e['ordinal'])
                _verify_counts((a, b), counts, edges)
                ax._eigentrail_edge_ids = tuple(e['ordinal'] for e in edges)
                if edges:
                    _draw_edges(ax, index, edges)
                else:
                    ax.text(0.5, 0.5, 'no candidate edges', ha='center', va='center',
                            transform=ax.transAxes, fontsize=8)
            else:
                ax.text(0.5, 0.45, 'edge rows not retained\n(projection: %s)\ncounts only'
                        % _projection_kind(result), ha='center', va='center',
                        transform=ax.transAxes, fontsize=8, style='italic')
    fig.suptitle('Subject %d window d%d (%s) -> d%d (%s)\n%s; graph range %s; outcome %s'
                 % (subject, from_date, dates[from_date], to_date, dates[to_date],
                    _mode_text(result), window['graph_range'], window['global_outcome']), fontsize=10)
    fig.text(0.5, 0.01, 'Rows/columns: source/destination lens. A agreement (solid); E exclusion (dotted); ? open (dashed); NA unavailable (dash-dot).\n'
             'Left/right: source/destination human ranks. Edge angles are rounded displays (≈), in degrees.',
             ha='center', fontsize=8)
    fig._eigentrail_counts = dict(window['global_counts'])
    fig.tight_layout(rect=(0, 0.06, 1, 0.94))
    return fig, axes


def _founder_records(result, query):
    origin, targets, unavailable = None, [], []
    for rec in result.records('founders'):
        if rec['query'] != query:
            continue
        if rec['kind'] == 'origin':
            origin = rec['origin']
        elif rec['kind'] == 'target':
            targets.append(rec)
        else:
            unavailable.append(rec)
    return origin, targets, unavailable


def _origin_text(origin, index):
    if origin is None:
        return 'origin: not recorded'
    if origin['kind'] == 'node':
        return 'origin: ' + _node_label(index, origin['node'])
    if origin['kind'] == 'internal_line':
        return 'origin: internal line of group %d' % origin['group']
    return 'origin: unavailable (%s)' % origin['reason']


def _bound(value):
    return value['degrees'] if value['kind'] == 'bound' else None


def _founder_row(info):
    return info['subject'], info['lens'], tuple(info['rank'])


def _founder_row_label(row, dimension):
    _, lens, rank = row
    kind = 'whole' if rank == (0, dimension) else 'line' if rank[1] - rank[0] == 1 else 'block'
    return '%s r%s (%s)' % (lens, _rank_label(rank), kind)


def _founder_bounds(lower, upper):
    lo, hi = _bound(lower), _bound(upper)
    if lo is None and hi is None:
        return 'no route'
    return '≈[%s, %s]°' % ('—' if lo is None else '%.3g' % lo,
                           '—' if hi is None else '%.3g' % hi)


@_without_tex
def plot_founder(result, query=0, *, family='typed_block_graph'):
    """Native reach/cost cells (top) and a union of selected typed paths (bottom).

    Rows retain lens and whole rank-group identity; no block midpoint is a line.
    All date positions and every target/path record are retained. For more than
    six dates, numeric labels appear only at the final date, up to twelve dates;
    longer histories use status markers and retain exact bounds in the canonical
    result on ``figure._eigentrail_result``. Only date tick labels are thinned.
    Axes retain target records, native parent IDs, original paths and drawn segment
    IDs for data-to-display checking. Returns ``(figure, (top, bottom))``.
    """
    result = _checked(result)
    plt = _pyplot()
    from matplotlib.lines import Line2D
    if family not in ('line_only', 'typed_block_graph', 'relaxed_internal_lines'):
        raise ValueError('unknown checked founder family')
    index = _node_index(result)
    dates = _date_labels(result)
    origin, targets, unavailable = _founder_records(result, query)
    dimension = len(result.request['features'])
    target_rows = []
    for rec in targets:
        state = next((f for f in rec['families'] if f['family'] == family), None)
        if state is None or rec['node'] not in index:
            raise ValueError('a checked target lost its typed node or requested family')
        target_rows.append((rec, state, index[rec['node']]))
    order = lambda row: (row[0], _LENSES.index(row[1]), row[2])
    typed_targets = sorted({_founder_row(info) for _, _, info in target_rows}, key=order)
    unavailable_lenses = sorted({rec['lens'] for rec in unavailable}, key=_LENSES.index)
    paths = [path for path in result.records('paths') if path['query'] == query and path['family'] == family]
    if any(node not in index for path in paths for node in path['nodes']):
        raise ValueError('a checked path lost a typed node')
    typed_paths = sorted({_founder_row(index[node]) for path in paths for node in path['nodes']}, key=order)
    rows = {row: i for i, row in enumerate(typed_targets)}
    path_rows = {row: i for i, row in enumerate(typed_paths)}
    top_count = len(typed_targets) + len(unavailable_lenses)
    height = 3.6 + 0.34 * (max(1, top_count) + max(1, len(typed_paths)))
    fig, (top, bottom) = plt.subplots(2, 1, figsize=(12, height), sharex=True,
        gridspec_kw={'height_ratios': (max(1, top_count) + 1, max(1, len(typed_paths)) + 1)},
        layout='constrained')
    fig.get_layout_engine().set(rect=(0, .11, 1, .82), h_pad=.08, w_pad=.08, hspace=.12)
    symbols = {'agreed': 'A', 'possible': '?', 'excluded': 'E', 'unavailable': 'NA'}
    colours = {'agreed': 'tab:green', 'possible': 'tab:orange', 'excluded': 'tab:red', 'unavailable': 'tab:gray'}
    markers = {'agreed': 'o', 'possible': '^', 'excluded': 'x', 'unavailable': 's'}
    annotated = len(dates) <= 6
    cells, target_data = {}, []
    for rec, state, info in target_rows:
        x, y, reach = info['date'], rows[_founder_row(info)], state['reach']['kind']
        direct = _STATUS[rec['direct']['verdict']] if rec['direct']['kind'] == 'available' else 'NA'
        target_data.append(dict(node=rec['node'], date=x, row=y, lower=state['lower'], upper=state['upper'],
            lower_parent=state['lower_parent'], upper_parent=state['upper_parent'], reach=state['reach'], direct=rec['direct']))
        if annotated or (len(dates) <= 12 and x == len(dates) - 1):
            bound_text = _founder_bounds(state['lower'], state['upper'])
            if not annotated:
                bound_text = bound_text.replace(', ', ',\n')
            cell = '%s  D:%s\n%s' % (symbols[reach], direct, bound_text)
            cells[rec['node']] = top.text(x, y, cell, ha='center', va='center', fontsize=7,
                color=colours[reach], bbox={'facecolor': 'white', 'edgecolor': 'none', 'pad': .15})
        else:
            top.plot(x, y - .13, marker=markers[reach], color=colours[reach], markersize=4, linestyle='none')
            direct_kind = {'A': 'agreed', 'E': 'excluded', '?': 'possible', 'NA': 'unavailable'}[direct]
            top.plot(x, y + .13, marker=markers[direct_kind], color=colours[direct_kind], markersize=3,
                markerfacecolor='none', linestyle='none')
    for rec in unavailable:
        y = len(typed_targets) + unavailable_lenses.index(rec['lens'])
        top.text(rec['date'], y, 'NA', ha='center', va='center', fontsize=7, color='tab:gray')
    top.set_yticks(range(top_count), [_founder_row_label(row, dimension) for row in typed_targets]
                  + [lens + ' (unavailable)' for lens in unavailable_lenses], fontsize=8)
    top.set_title('Native reach: A agreed · ? possible · E excluded · NA unavailable\n'
                  'D direct A/E/?/NA; ≈[lower, upper]°', fontsize=8)
    top.set_ylabel('typed target')
    top._eigentrail_target_nodes = tuple(rec['node'] for rec, _, _ in target_rows)
    top._eigentrail_target_records = tuple(target_data)
    top._eigentrail_unavailable_records = tuple(unavailable)
    top._eigentrail_cell_texts = cells
    top._eigentrail_rows = tuple(typed_targets)
    segments = {(path['side'], source, target) for path in paths
                for source, target in zip(path['nodes'], path['nodes'][1:])}
    for side, source, target in sorted(segments, key=lambda item: (item[0] == 'lower', item[1], item[2])):
        line, = bottom.plot([index[source]['date'], index[target]['date']],
            [path_rows[_founder_row(index[source])], path_rows[_founder_row(index[target])]],
            linestyle='-' if side == 'upper' else ':', color='tab:blue' if side == 'upper' else '0.25',
            linewidth=1.2 if side == 'upper' else 1)
        line._eigentrail_segment = side, source, target
    for node in sorted({node for path in paths for node in path['nodes']}):
        info = index[node]
        shape = 'D' if info['rank'] == [0, dimension] else 'o' if info['rank'][1] - info['rank'][0] == 1 else 's'
        bottom.plot(info['date'], path_rows[_founder_row(info)], marker=shape, color='black',
                    markerfacecolor='white', markersize=4, linestyle='none')
    if not paths:
        bottom.text(0.5, 0.5, 'no checked selected paths for this family', ha='center',
                    transform=bottom.transAxes, fontsize=8, style='italic')
    if not target_rows and not unavailable:
        top.text(.5, .5, 'no checked targets for this query', ha='center', transform=top.transAxes, fontsize=8)
    bottom.set_yticks(range(len(typed_paths)), [_founder_row_label(row, dimension) for row in typed_paths], fontsize=8)
    bottom._eigentrail_path_nodes = tuple(tuple(path['nodes']) for path in paths)
    bottom._eigentrail_path_records = tuple(paths)
    bottom._eigentrail_segments = tuple(sorted(segments))
    bottom._eigentrail_rows = tuple(typed_paths)
    bottom.set_title('Selected native path union; separate typed vertices and original date gaps', fontsize=8)
    bottom.set_ylabel('typed path node')
    bottom.set_xlabel('original date index and ID')
    bottom.set_xticks(range(len(dates)))
    step = max(1, (len(dates) + 6) // 7)
    labelled = set(range(0, len(dates), step))
    if labelled and len(dates) - 1 not in labelled and len(dates) - 1 - max(labelled) < step:
        labelled.remove(max(labelled))
    labelled.add(len(dates) - 1)
    bottom.set_xticklabels(['%d\n%s' % (i, date) if i in labelled else ''
                           for i, date in enumerate(dates)], fontsize=7)
    bottom.set_xlim(-0.5, len(dates) - 0.5)
    for axis, count in ((top, top_count), (bottom, len(typed_paths))):
        axis.set_ylim(max(1, count) - .5, -.5)
        axis.set_axisbelow(True)
        axis.grid(color='0.9', linewidth=.5)
        axis.tick_params(axis='both', length=2)
    handles = [Line2D([], [], color='tab:blue', linestyle='-', label='upper path'),
               Line2D([], [], color='0.25', linestyle=':', label='lower path')]
    handles.extend(Line2D([], [], color='black', marker=shape, markerfacecolor='white', linestyle='none', label=label)
                   for shape, label in (('o', 'line'), ('s', 'block'), ('D', 'whole space')))
    fig.legend(handles=handles, loc='lower center', bbox_to_anchor=(.5, .038), ncol=5, fontsize=7, frameon=False)
    mode = result.request['uncertainty']['mode']
    fig.suptitle('Founder query %d · %s · tolerance ≈%.6g°\n%s · mode=%s · lag cap=%d'
        % (query, family, result.request['tolerance_deg'], _origin_text(origin, index), mode,
           result.request['scope']['max_lag']), fontsize=9)
    note = '≈ denotes rounded native degrees; exact costs, parents, IDs and all paths remain in the canonical report.'
    if not annotated:
        note += ('\nBounds labeled at the final date only; ' if len(dates) <= 12 else
                 '\nNumeric labels omitted above 12 dates; ') + 'upper/lower cell markers show reach/direct verdict (○ A, △ ?, × E, □ NA).'
    fig.text(.5, .012, note, ha='center', va='bottom', fontsize=7)
    fig._eigentrail_result = result
    return fig, (top, bottom)


@_without_tex
def plot_spectrum(result, *, lens='covariance', subject=0):
    """Checked spectrum enclosures per date and human rank; grouped ties are annotated.

    The ``partial`` lens is plotted on a signed axis with a zero line. Axes carry
    ``_eigentrail_views`` as a tuple of (date, reference) pairs.
    """
    result = _checked(result)
    if lens not in _LENSES:
        raise ValueError('unknown lens')
    plt = _pyplot()
    dates = _date_labels(result)
    fig, ax = plt.subplots(figsize=(12, 6))
    seen = []
    for view in result.records('views'):
        if view['subject'] != subject or view['lens'] != lens:
            continue
        x = view['date']
        seen.append((x, view['reference']))
        spectrum = view['spectrum']
        if spectrum['kind'] != 'available':
            ax.text(x, 0, '%s: %s' % (spectrum['kind'], spectrum['reason']), rotation=90,
                    fontsize=7, ha='center', style='italic', transform=ax.get_xaxis_transform())
            continue
        intervals = spectrum['intervals']
        for rank, (lo, hi) in enumerate(intervals, start=1):
            ax.plot([x, x], [lo, hi], color='black', linewidth=3, solid_capstyle='butt')
            ax.text(x + 0.06, (lo + hi) / 2, str(rank), fontsize=6, va='center')
        for group in view['groups']:
            lo_r, hi_r = group['rank']
            if hi_r - lo_r > 1:
                vals = [v for pair in intervals[lo_r:hi_r] for v in pair]
                ax.plot([x - 0.15, x - 0.15], [min(vals), max(vals)], color='tab:blue', linewidth=1)
                ax.text(x - 0.18, max(vals), 'group r%s (%s)' % (_rank_label(group['rank']), group['grouping']),
                        fontsize=6, ha='right', color='tab:blue')
    if not seen:
        ax.text(0.5, 0.5, 'no checked %s view records retained' % lens, ha='center',
                transform=ax.transAxes, style='italic')
    if lens == 'partial':
        ax.axhline(0, color='gray', linewidth=0.8)
        ax.set_ylabel('partial-correlation lens eigenvalue enclosure (signed)')
    else:
        ax.set_ylabel('%s lens eigenvalue enclosure' % lens)
    ax.set_xticks(range(len(dates)))
    ax.set_xticklabels(['%d\n%s' % (i, d) for i, d in enumerate(dates)], fontsize=7)
    ax.set_xlim(-0.5, len(dates) - 0.5)
    ax.set_xlabel('date index (id)')
    ax.set_title('Subject %d %s spectrum (numbers: human ranks; blue: retained groups)\n%s'
                 % (subject, lens, _mode_text(result)), fontsize=9)
    ax._eigentrail_views = tuple(seen)
    fig.tight_layout()
    return fig, ax


_SCRIPT = """
(function(){var f=document.getElementById('edge-filter');var rows=document.querySelectorAll('#edges tbody tr');
var s=document.getElementById('edge-status');function run(){var q=f.value.toLowerCase();var n=0;
rows.forEach(function(r){var m=r.textContent.toLowerCase().indexOf(q)>=0;r.hidden=!m;if(m)n++;});
s.textContent=n+' of '+rows.length+' edge rows shown';}f.addEventListener('input',run);run();})();
"""
_STYLE_CSS = """body{font-family:sans-serif;margin:1.5em}table{border-collapse:collapse;font-size:13px}
td,th{border:1px solid #999;padding:2px 6px}input:focus{outline:3px solid #1a5fb4;outline-offset:2px}
a:focus{outline:3px solid #1a5fb4}pre{max-height:20em;overflow:auto;background:#f4f4f4;padding:.5em}"""


def _edge_rows(result, index):
    esc = html.escape
    out = []
    for edge in result.records('edges'):
        src, dst = index.get(edge['source'], {}), index.get(edge['target'], {})
        cells = [edge['ordinal'], src.get('subject', '?'),
                 _node_label(index, edge['source']), _node_label(index, edge['target']),
                 src.get('date', '?'), dst.get('date', '?'), edge['edge_type'], edge['smaller_side'],
                 _enclosure(edge['point']), _enclosure(edge['geometry']),
                 _enclosure(edge['method_bracket']),
                 '%s (%s)' % (_STATUS[edge['verdict']], edge['verdict'])]
        out.append('<tr>' + ''.join('<td>%s</td>' % esc(str(c)) for c in cells) + '</tr>')
    return out


def _text(value):
    return value.decode('utf-8') if isinstance(value, (bytes, bytearray)) else str(value)


def _download(name, text):
    data = base64.b64encode(text.encode('utf-8')).decode('ascii')
    return '<a download="%s" href="data:application/json;base64,%s">Download %s</a>' % (
        html.escape(name), data, html.escape(name))


def export_html(result, path):
    """Write a fully offline, searchable edge table; the destination must not exist.

    Contains no network references. Canonical request and report JSON are embedded
    verbatim (escaped in <pre> and as data-URI downloads). Returns the Path.
    """
    result = _checked(result)
    path = Path(path)
    index = _node_index(result)
    esc = html.escape
    windows = []
    for w in result.records('windows'):
        windows.append('<tr><td>%s</td><td>%s</td><td>%s</td><td>%s</td><td>%s</td></tr>' % tuple(
            esc(str(v)) for v in (w['subject'], '%d -> %d' % (w['from_date'], w['to_date']),
                                  w['global_outcome'], _count_text(w['global_counts']), w['graph_range'])))
    rows = _edge_rows(result, index)
    request_text, report_text = _text(result.request_json), _text(result.report_json)
    heads = ['ordinal', 'subject', 'source', 'destination', 'from date', 'to date', 'type',
             'smaller side', 'point', 'geometry', 'method bracket', 'status']
    body = ''.join([
        '<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">',
        '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; ',
        'style-src \'unsafe-inline\'; script-src \'unsafe-inline\'; img-src data:">',
        '<title>eigentrail rolling lineage report</title><style>', _STYLE_CSS, '</style></head><body>',
        '<h1>eigentrail rolling lineage report</h1>',
        '<p>Request hash: <code>%s</code>; %s; projection: %s</p>' % (
            esc(str(result['request_hash'])), esc(_mode_text(result)), esc(_projection_kind(result))),
        '<p>Protocol: %s</p>' % esc(str(result['protocol'])),
        '<h2>Windows</h2><table><caption>Checked window outcomes</caption><thead><tr><th scope="col">subject</th><th scope="col">dates</th>',
        '<th scope="col">outcome</th><th scope="col">counts</th><th scope="col">graph range</th></tr></thead><tbody>',
        ''.join(windows), '</tbody></table>',
        '<h2>Edges</h2>',
        '<p>Status: A agreement, E exclusion, ? open, NA unavailable.</p>',
        '<label for="edge-filter">Filter edge rows (any text): </label>',
        '<input id="edge-filter" type="search" autocomplete="off" aria-controls="edges" aria-describedby="edge-status">',
        '<p id="edge-status" role="status" aria-live="polite">%d edge rows</p>' % len(rows),
        '<table id="edges"><caption>Selected checked edge records</caption><thead><tr>', ''.join('<th scope="col">%s</th>' % esc(h) for h in heads),
        '</tr></thead><tbody>', ''.join(rows) or '', '</tbody></table>',
        '' if rows else '<p>No edge rows were retained by the projection; see window counts.</p>',
        '<h2>Canonical request</h2><p>', _download('request.json', request_text), '</p>',
        '<pre>', esc(request_text), '</pre>',
        '<h2>Canonical report</h2><p>', _download('report.json', report_text), '</p>',
        '<pre>', esc(report_text), '</pre>',
        '<script>', _SCRIPT, '</script></body></html>'])
    with open(path, 'x', encoding='utf-8', newline='\n') as handle:
        handle.write(body)
    return path
