// SPDX-License-Identifier: AGPL-3.0-only
//! Source model-1/keyvalues2 DMX, one mesh per input. Several QC bodies still
//! compile into ONE MDL. This avoids `StudioMDL`'s fragile large-DMX mesh importer.
use crate::region::{RegionMesh, Triangle};
use anyhow::Result;
use core::fmt::Write as _;
use std::{fs, path::Path};
/// Maximum native triangles per DMX body to avoid importer limits.
const BODY_TRIANGLES: usize = 2400;
/// Keyvalues2 model-format DMX text builder.
struct Dmx {
    /// Accumulated DMX document.
    text: String,
    /// Body number used to keep element IDs unique.
    body: usize,
}
#[allow(clippy::unwrap_used, reason = "fmt::Write for String is infallible")]
impl Dmx {
    /// Generate a deterministic element UUID scoped to this body.
    fn uid(&self, i: usize) -> String {
        format!("00000000-0000-0000-{:04x}-{i:012x}", self.body)
    }
    /// Open a typed DMX element.
    fn begin(&mut self, kind: &str, id: usize, name: &str) {
        let uid = self.uid(id);
        writeln!(
            self.text,
            "\"{kind}\"\n{{\n\"id\" \"elementid\" \"{uid}\"\n\"name\" \"string\" \"{name}\""
        )
        .unwrap();
    }
    /// Append a typed scalar field.
    fn field(&mut self, key: &str, kind: &str, value: &str) {
        writeln!(self.text, "\"{key}\" \"{kind}\" \"{value}\"").unwrap();
    }
    /// Append a reference to another element.
    fn reference(&mut self, key: &str, id: usize) {
        self.field(key, "element", &self.uid(id));
    }
    /// Append an array of element references.
    fn refs(&mut self, key: &str, ids: &[usize]) {
        writeln!(self.text, "\"{key}\" \"element_array\"\n[").unwrap();
        for (i, id) in ids.iter().enumerate() {
            let uid = self.uid(*id);
            writeln!(
                self.text,
                "\"element\" \"{uid}\"{}",
                if i.saturating_add(1) < ids.len() {
                    ","
                } else {
                    ""
                }
            )
            .unwrap();
        }
        self.text.push_str("]\n");
    }
    /// Close the current DMX element.
    fn end(&mut self) {
        self.text.push_str("}\n");
    }
    /// Write an identity position/orientation transform.
    fn transform(&mut self, id: usize, name: &str) {
        self.begin("DmeTransform", id, name);
        self.field("position", "vector3", "0 0 0");
        self.field("orientation", "quaternion", "0 0 0 1");
        self.end();
    }
    /// Append a typed DMX array.
    #[allow(
        clippy::shadow_reuse,
        reason = "local rebinding records successive numeric representations"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn array(&mut self, name: &str, kind: &str, values: impl IntoIterator<Item = String>) {
        writeln!(self.text, "\"{name}\" \"{kind}_array\"\n[").unwrap();
        let mut values = values.into_iter().peekable();
        while let Some(v) = values.next() {
            writeln!(
                self.text,
                "\"{v}\"{}",
                if values.peek().is_some() { "," } else { "" }
            )
            .unwrap();
        }
        self.text.push_str("]\n");
    }
}
/// Serialize native triangles as a root-bound DMX mesh body.
#[allow(
    clippy::too_many_lines,
    reason = "keep native opcode dispatch and ordered conversion stages together"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
pub(crate) fn body(triangles: &[Triangle], id: &str, number: usize) -> String {
    let mut d = Dmx {
        text: "<!-- dmx encoding keyvalues2 1 format model 1 -->\n".into(),
        body: number,
    };
    d.begin("DmElement", 1, "root");
    d.reference("skeleton", 2);
    d.reference("model", 2);
    d.end();
    d.begin("DmeModel", 2, "reference");
    d.reference("transform", 3);
    d.refs("children", &[1000, 5]);
    d.refs("jointTransforms", &[1001, 6]);
    d.refs("baseStates", &[4]);
    d.end();
    d.transform(3, "model_transform");
    d.begin("DmeTransformList", 4, "bind");
    d.refs("transforms", &[1002, 8]);
    d.end();
    d.begin("DmeJoint", 1000, "terrain_root");
    d.reference("transform", 1001);
    d.refs("children", &[]);
    d.end();
    d.transform(1001, "terrain_root");
    d.transform(1002, "terrain_root");
    d.begin("DmeDag", 5, "terrain");
    d.reference("transform", 6);
    d.reference("shape", 7);
    d.end();
    d.transform(6, "mesh_transform");
    d.transform(8, "mesh_transform");
    let materials = triangles
        .iter()
        .map(|t| t.material)
        .collect::<std::collections::BTreeSet<_>>();
    d.begin("DmeMesh", 7, "terrain");
    d.field("visible", "bool", "1");
    d.reference("bindState", 9);
    d.reference("currentState", 9);
    d.refs("baseStates", &[9]);
    d.refs(
        "faceSets",
        &materials
            .iter()
            .map(|index| 20_usize.saturating_add(index.saturating_mul(2)))
            .collect::<Vec<_>>(),
    );
    d.end();
    let verts = triangles
        .iter()
        .flat_map(|t| t.vertices)
        .collect::<Vec<_>>();
    let count = verts.len();
    d.begin("DmeVertexData", 9, "bind");
    d.array(
        "vertexFormat",
        "string",
        [
            "positions",
            "normals",
            "textureCoordinates",
            "jointWeights",
            "jointIndices",
        ]
        .map(str::to_owned),
    );
    d.field("flipVCoordinates", "bool", "0");
    d.field("jointCount", "int", "1");
    d.array(
        "positions",
        "vector3",
        verts
            .iter()
            .map(|v| format!("{} {} {}", v.pos[0], v.pos[1], v.pos[2])),
    );
    d.array("positionsIndices", "int", (0..count).map(|i| i.to_string()));
    d.array(
        "normals",
        "vector3",
        verts
            .iter()
            .map(|v| format!("{} {} {}", v.normal[0], v.normal[1], v.normal[2])),
    );
    d.array("normalsIndices", "int", (0..count).map(|i| i.to_string()));
    d.array(
        "textureCoordinates",
        "vector2",
        triangles.iter().flat_map(|t| {
            t.vertices
                .iter()
                .map(move |_| format!("{} {}", t.blend_address, t.layer))
        }),
    );
    d.array(
        "textureCoordinatesIndices",
        "int",
        (0..count).map(|i| i.to_string()),
    );
    d.array(
        "jointWeights",
        "float",
        core::iter::repeat_with(|| "1".into()).take(count),
    );
    d.array(
        "jointIndices",
        "int",
        core::iter::repeat_with(|| "0".into()).take(count),
    );
    d.end();
    for material in materials {
        let base = 20_usize.saturating_add(material.saturating_mul(2));
        let name = format!("{id}_{material}");
        d.begin("DmeFaceSet", base, &name);
        d.reference("material", base.saturating_add(1));
        d.array(
            "faces",
            "int",
            triangles
                .iter()
                .enumerate()
                .filter(|&(_, t)| t.material == material)
                .flat_map(|(i, _)| {
                    [
                        i.saturating_mul(3).to_string(),
                        i.saturating_mul(3).saturating_add(1).to_string(),
                        i.saturating_mul(3).saturating_add(2).to_string(),
                        "-1".to_owned(),
                    ]
                }),
        );
        d.end();
        d.begin("DmeMaterial", base.saturating_add(1), &name);
        d.field("mtlName", "string", &name);
        d.end();
    }
    d.text
}
/// Write DMX mesh bodies and the QC that compiles them into one model.
pub(crate) fn write(baked: &RegionMesh, source: &Path, namespace: &str, id: &str) -> Result<()> {
    let mut qc=format!("$modelname \"garryscape/{namespace}/regions/{id}.mdl\"\n$cdmaterials \"models/garryscape/{namespace}/regions/\"\n$upaxis Z\n$staticprop\n$surfaceprop dirt\n");
    for (i, triangles) in baked.triangles.chunks(BODY_TRIANGLES).enumerate() {
        fs::write(
            source.join(format!("reference_{i}.dmx")),
            body(triangles, id, i),
        )?;
        writeln!(qc, "$body terrain_{i} \"reference_{i}.dmx\"")?;
    }
    qc.push_str("$sequence idle \"reference_0.dmx\" fps 1\n");
    fs::write(source.join("model.qc"), qc)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Vertex;
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    fn body_ids_are_unique_and_no_collision() {
        let v = Vertex {
            pos: [0.; 3],
            normal: [0., 0., 1.],
            uv: [0.; 2],
        };
        let t = Triangle {
            vertices: [v; 3],
            water: false,
            material: 0,
            underwater: false,
            layer: 0,
            blend_address: 0,
        };
        let a = body(core::slice::from_ref(&t), "50_50", 0);
        let b = body(&[t], "50_50", 1);
        assert!(a.contains("format model 1"));
        assert!(!b.contains("00000000-0000-0000-0000-"));
        assert!(!a.contains("collision"));
        assert!(!a.contains("50_50_water"));
    }
}
