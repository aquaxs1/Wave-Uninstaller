/*
 * Wave Uninstaller — what a removal looks like.
 *
 * Leftovers float on a sea: folders, registry keys, startup entries. A wave
 * runs across and carries each one into the backup crate — moved, not
 * deleted. A moment later they all come back: that is the undo. Click or tap
 * to trigger the wave, or the undo, yourself. The water answers the pointer.
 */
export default function scene(stage) {
  const { THREE, frame } = stage;
  stage.lights({ ambient: 0.5, key: 2.2, rim: 22 });
  stage.environment(0.7);

  const root = new THREE.Group();
  frame.add(root);

  /* --------------------------------------------------------------- sea */

  const WIDTH = 6.6;
  const DEPTH = 5;
  const uniforms = {
    uTime: { value: 0 },
    uSweep: { value: -9 },
    uPointer: { value: new THREE.Vector2(99, 99) },
    uColor: { value: stage.accent.clone() },
    uHi: { value: stage.accentHi.clone() },
    uSize: { value: 26 },
  };

  // The same height, once for the GPU and once (below) for what floats on it.
  const heightGLSL = `
    uniform float uTime;
    uniform float uSweep;
    uniform vec2 uPointer;
    float seaHeight(vec2 p) {
      float h = sin(p.x * 1.15 + uTime * 0.9) * 0.11
              + sin(p.y * 1.8 - uTime * 0.75) * 0.07
              + sin((p.x + p.y) * 0.62 + uTime * 0.5) * 0.09;
      h += exp(-pow((p.x - uSweep) * 1.25, 2.0)) * 0.62;
      float d = distance(p, uPointer);
      h += exp(-d * d * 1.5) * 0.22 * sin(d * 6.0 - uTime * 5.0);
      return h;
    }`;
  const seaHeight = (x, y, time, sweep) =>
    Math.sin(x * 1.15 + time * 0.9) * 0.11 +
    Math.sin(y * 1.8 - time * 0.75) * 0.07 +
    Math.sin((x + y) * 0.62 + time * 0.5) * 0.09 +
    Math.exp(-Math.pow((x - sweep) * 1.25, 2)) * 0.62;

  const vertexShader = `
    ${heightGLSL}
    uniform float uSize;
    varying float vHeight;
    varying vec2 vUv;
    void main() {
      vUv = uv;
      vec3 p = position;
      vHeight = seaHeight(p.xy);
      p.z += vHeight;
      vec4 mv = modelViewMatrix * vec4(p, 1.0);
      gl_Position = projectionMatrix * mv;
      gl_PointSize = uSize * (1.0 + vHeight * 1.8) / -mv.z;
    }`;
  const fragmentShader = (points) => `
    uniform vec3 uColor;
    uniform vec3 uHi;
    varying float vHeight;
    varying vec2 vUv;
    void main() {
      ${points ? 'if (length(gl_PointCoord - 0.5) > 0.5) discard;' : ''}
      float edge = smoothstep(0.0, 0.24, vUv.x) * smoothstep(1.0, 0.76, vUv.x)
                 * smoothstep(0.0, 0.3, vUv.y) * smoothstep(1.0, 0.7, vUv.y);
      float lift = smoothstep(-0.15, 0.6, vHeight);
      vec3 c = mix(uColor * 0.5, uHi, lift);
      gl_FragColor = vec4(c, edge * (${points ? '0.25 + lift * 0.75' : '0.07 + lift * 0.3'}));
    }`;

  const detail = stage.quality < 1 ? 0.6 : 1;
  const plane = new THREE.PlaneGeometry(WIDTH, DEPTH, Math.round(120 * detail), Math.round(76 * detail));
  const sea = new THREE.Group();
  sea.rotation.x = -Math.PI / 2;
  sea.position.set(0.25, -0.9, 0);
  const common = { uniforms, vertexShader, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending };
  sea.add(new THREE.Mesh(plane, new THREE.ShaderMaterial({ ...common, fragmentShader: fragmentShader(false), wireframe: true })));
  sea.add(new THREE.Points(plane, new THREE.ShaderMaterial({ ...common, fragmentShader: fragmentShader(true) })));
  root.add(sea);

  /* ------------------------------------------------------------- crate */

  const CRATE = 1.35;
  const crate = new THREE.Group();
  crate.position.set(1.85, 0.8, -0.4);
  const crateBox = new THREE.BoxGeometry(CRATE, CRATE, CRATE);
  crate.add(new THREE.Mesh(crateBox, stage.mat.glass(stage.accent, { opacity: 0.08 })));
  const crateEdges = stage.edges(crateBox, stage.accentHi, 0.8);
  crate.add(crateEdges);
  const label = stage.plate(1.25, 0.2, (g, w, h) => {
    g.fillStyle = `#${stage.accentHi.getHexString()}`;
    g.font = stage.font.mono(Math.round(h * 0.5));
    g.textBaseline = 'middle';
    g.textAlign = 'center';
    g.fillText('%ProgramData%\\Wave\\Backups', w / 2, h / 2);
  });
  label.position.set(0, -CRATE / 2 - 0.2, CRATE / 2);
  crate.add(label);
  crate.add(stage.halo(stage.accent, 3.4, 0.3));
  root.add(crate);

  /* --------------------------------------------------------- leftovers */

  const shapes = [
    new THREE.BoxGeometry(0.24, 0.24, 0.24), // a registry key
    stage.slab(0.42, 0.3, 0.07, 0.05), // a folder
    new THREE.BoxGeometry(0.32, 0.08, 0.22), // a startup entry
  ];
  const tints = [0xf2f6fa, 0xcfeeff, 0x9fdcff];
  const COUNT = 24;
  const items = [];
  for (let i = 0; i < COUNT; i += 1) {
    const mesh = new THREE.Mesh(
      shapes[i % 3],
      stage.mat.solid(tints[i % 3], { roughness: 0.4, emissive: stage.accent, emissiveIntensity: 0.12 }),
    );
    // A stable scatter, so the picture is the same on every visit.
    const u = (i * 0.618034) % 1;
    const v = (i * 0.414214 + 0.13) % 1;
    const home = new THREE.Vector3(-2.35 + u * 3.3, 0, -1.6 + v * 3.2);
    const slot = new THREE.Vector3(((i % 3) - 1) * 0.36, (Math.floor(i / 9) - 1) * 0.36, ((Math.floor(i / 3) % 3) - 1) * 0.36);
    root.add(mesh);
    items.push({ mesh, home, slot, amount: 0, target: 0, seed: i * 2.39 });
  }

  stage.dust({ count: 200, radius: 7, color: stage.accentHi, opacity: 0.5 });

  /* --------------------------------------------------------- behaviour */

  // One cycle: calm, the wave crosses, everything rests in the crate, undo.
  const CYCLE = 13;
  let clock = 0;

  stage.onClick(() => {
    const stored = items.filter((item) => item.target === 1).length;
    clock = stored > COUNT / 2 ? 8.4 : 1.4;
  });

  const tmp = new THREE.Vector3();
  const place = new THREE.Vector3();
  const UP = new THREE.Vector3(0, 1, 0);

  stage.onFrame((time, delta) => {
    clock = (clock + delta) % CYCLE;

    const sweep = clock > 1.5 && clock < 5.6 ? -4.2 + ((clock - 1.5) / 4.1) * 8.4 : -9;
    uniforms.uTime.value = time;
    uniforms.uSweep.value = sweep - sea.position.x;
    if (stage.pointer.inside && !stage.coarse) {
      // Pointer in the sea's own plane: rough, but the ripple follows the hand.
      uniforms.uPointer.value.set(((stage.pointer.x * stage.view.width) / 2 - frame.position.x) / frame.scale.x - sea.position.x, stage.pointer.y * (DEPTH / 2));
    } else {
      uniforms.uPointer.value.set(99, 99);
    }

    root.rotation.x = 0.36 + stage.pitch * 0.6;
    root.rotation.y = -0.28 + stage.yaw * 0.6 + Math.sin(time * 0.2) * 0.04;
    root.position.y = -0.1;

    crateEdges.material.opacity = 0.55 + Math.sin(time * 2) * 0.1;
    crate.rotation.y = Math.sin(time * 0.4) * 0.12;
    crate.position.y = 0.8 + Math.sin(time * 0.9) * 0.05;

    for (const item of items) {
      if (clock > 8.5) item.target = 0;
      else if (sweep > item.home.x - 0.15) item.target = 1;

      const speed = item.target ? 1.5 : 1.1 + (item.seed % 0.6);
      item.amount += (item.target - item.amount) * Math.min(1, delta * speed);
      const s = item.amount * item.amount * (3 - 2 * item.amount);

      const floatY = sea.position.y + seaHeight(item.home.x - sea.position.x, -item.home.z, time, sweep - sea.position.x) + 0.16;
      tmp.set(item.home.x, floatY, item.home.z);
      // Its place in the crate moves with the crate.
      place.copy(item.slot).applyAxisAngle(UP, crate.rotation.y).add(crate.position);
      item.mesh.position.lerpVectors(tmp, place, s);
      item.mesh.position.y += Math.sin(Math.PI * s) * 1.15;
      const loose = 1 - s;
      item.mesh.rotation.set(
        (time * 0.4 + item.seed) * loose,
        (time * 0.3 + item.seed * 1.3) * loose,
        Math.sin(time * 0.8 + item.seed) * 0.4 * loose,
      );
      item.mesh.scale.setScalar(1 - s * 0.35);
    }
  });
}
