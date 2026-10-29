// Groups ruling lines into tables with a grid of cells.
use super::rules::Rule;

#[derive(Debug, Clone, PartialEq)]
pub struct CellBox {
    pub bbox: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub bbox: [f64; 4],
    pub rows: Vec<Vec<CellBox>>,
}

// Slack when deciding that two lines touch, in pixels, on top of their
// combined thickness.
const TOUCH: f64 = 6.0;

fn touches(h: &Rule, v: &Rule) -> bool {
    let tol = TOUCH + h.width + v.width;
    v.x1 >= h.x1 - tol && v.x1 <= h.x2 + tol && h.y1 >= v.y1 - tol && h.y1 <= v.y2 + tol
}

// Connected components over the "touches" graph of horizontal and
// vertical rules; returns the component id of each rule.
fn components(hs: &[&Rule], vs: &[&Rule]) -> (Vec<usize>, Vec<usize>) {
    let n = hs.len() + vs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (i, h) in hs.iter().enumerate() {
        for (j, v) in vs.iter().enumerate() {
            if touches(h, v) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, hs.len() + j));
                parent[a] = b;
            }
        }
    }
    let ids: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    (ids[..hs.len()].to_vec(), ids[hs.len()..].to_vec())
}

// Distinct positions, merging values closer than `tol`.
fn positions(mut vals: Vec<f64>, tol: f64) -> Vec<f64> {
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out: Vec<(f64, usize)> = Vec::new();
    for v in vals {
        match out.last_mut() {
            Some((mean, n)) if (v - *mean).abs() <= tol => {
                *mean = (*mean * *n as f64 + v) / (*n as f64 + 1.0);
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out.into_iter().map(|(m, _)| m).collect()
}

// Whether some vertical rule covers the boundary x=`x` across most of
// the band y0..y1.
fn separated(vs: &[&Rule], x: f64, y0: f64, y1: f64, tol: f64) -> bool {
    let need = 0.5 * (y1 - y0);
    vs.iter()
        .any(|v| (v.x1 - x).abs() <= tol && (v.y2.min(y1) - v.y1.max(y0)) >= need)
}

fn build(hs: &[&Rule], vs: &[&Rule]) -> Option<Table> {
    let tol = TOUCH + hs.iter().chain(vs).map(|r| r.width).fold(0.0, f64::max);
    let ys = positions(hs.iter().map(|r| r.y1).collect(), tol);
    let xs = positions(vs.iter().map(|r| r.x1).collect(), tol);
    if ys.len() < 2 || xs.len() < 2 {
        return None;
    }
    let mut rows = Vec::new();
    for pair in ys.windows(2) {
        let (y0, y1) = (pair[0], pair[1]);
        let mut row: Vec<CellBox> = Vec::new();
        for (j, cols) in xs.windows(2).enumerate() {
            let cell = CellBox {
                bbox: [cols[0], y0, cols[1], y1],
            };
            // No line between this cell and the previous one: a colspan.
            if j > 0 && !separated(vs, cols[0], y0, y1, tol) {
                row.last_mut().unwrap().bbox[2] = cols[1];
            } else {
                row.push(cell);
            }
        }
        rows.push(row);
    }
    Some(Table {
        bbox: [xs[0], ys[0], xs[xs.len() - 1], ys[ys.len() - 1]],
        rows,
    })
}

// Extracts tables from `rules`, leaving the rules that belong to none.
pub fn find_tables(rules: &mut Vec<Rule>) -> Vec<Table> {
    let hs: Vec<&Rule> = rules.iter().filter(|r| r.is_horizontal()).collect();
    let vs: Vec<&Rule> = rules.iter().filter(|r| !r.is_horizontal()).collect();
    let (hid, vid) = components(&hs, &vs);
    let mut tables = Vec::new();
    let mut used: Vec<Rule> = Vec::new();
    let mut comps: Vec<usize> = hid.iter().chain(&vid).copied().collect();
    comps.sort_unstable();
    comps.dedup();
    for c in comps {
        let ch: Vec<&Rule> = hs
            .iter()
            .zip(&hid)
            .filter(|(_, &id)| id == c)
            .map(|(r, _)| *r)
            .collect();
        let cv: Vec<&Rule> = vs
            .iter()
            .zip(&vid)
            .filter(|(_, &id)| id == c)
            .map(|(r, _)| *r)
            .collect();
        if ch.len() < 2 || cv.len() < 2 {
            continue;
        }
        if let Some(t) = build(&ch, &cv) {
            tables.push(t);
            used.extend(ch.into_iter().chain(cv).cloned());
        }
    }
    rules.retain(|r| !used.contains(r));
    tables.sort_by(|a, b| a.bbox[1].partial_cmp(&b.bbox[1]).unwrap());
    tables
}

#[cfg(test)]
mod tests {
    use super::super::rules::{find_rules, testing::grid};
    use super::*;

    fn h(x1: f64, x2: f64, y: f64) -> Rule {
        Rule {
            x1,
            y1: y,
            x2,
            y2: y,
            width: 2.0,
        }
    }

    fn v(x: f64, y1: f64, y2: f64) -> Rule {
        Rule {
            x1: x,
            y1,
            x2: x,
            y2,
            width: 2.0,
        }
    }

    #[test]
    fn groups_grid_into_three_by_four() {
        let img = grid(
            800,
            600,
            &[100, 250, 400, 550, 700],
            &[100, 200, 300, 400],
            3,
        );
        let mut rules = find_rules(&img);
        let tables = find_tables(&mut rules);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.rows.len(), 3);
        assert!(t.rows.iter().all(|r| r.len() == 4), "{t:?}");
        assert_eq!(t.rows.iter().map(|r| r.len()).sum::<usize>(), 12);
        assert!((t.bbox[0] - 101.5).abs() < 2.0 && (t.bbox[3] - 401.5).abs() < 2.0);
        assert!(rules.is_empty(), "leftover {rules:?}");
    }

    #[test]
    fn keeps_free_rules_and_merges_colspans() {
        let mut rules = vec![
            h(0.0, 300.0, 0.0),
            h(0.0, 300.0, 50.0),
            h(0.0, 300.0, 100.0),
            v(0.0, 0.0, 100.0),
            v(300.0, 0.0, 100.0),
            v(150.0, 50.0, 100.0),
            // A lone underline far away.
            h(500.0, 700.0, 400.0),
        ];
        let tables = find_tables(&mut rules);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows[0].len(), 1);
        assert_eq!(tables[0].rows[1].len(), 2);
        assert_eq!(tables[0].rows[0][0].bbox, [0.0, 0.0, 300.0, 50.0]);
        assert_eq!(rules, vec![h(500.0, 700.0, 400.0)]);
    }
}
