import * as THREE from "three"
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js"
import { RenderPass } from "three/addons/postprocessing/RenderPass.js"
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js"
import { OutputPass } from "three/addons/postprocessing/OutputPass.js"
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js"

export interface ShowcaseScene {
  setPaused(paused: boolean): void
  selectStep(step: number): void
  dispose(): void
}

interface SceneOptions {
  paused: boolean
  onStep(step: number): void
  onUnavailable(): void
}

const vertexShader = /* glsl */`
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`

// A shared, moving landscape makes the input/output quality metaphor legible.
// This is an illustration, not a claim to run video enhancement in the browser.
const landscapeShader = /* glsl */`
  varying vec2 vUv;
  uniform float uTime;
  uniform float uCoarse;
  uniform float uReveal;
  float hash(float n) { return fract(sin(n) * 43758.5453); }
  float noise(float x) {
    float i = floor(x), f = fract(x);
    return mix(hash(i), hash(i + 1.0), f * f * (3.0 - 2.0 * f));
  }
  void main() {
    vec2 uv = vUv;
    bool coarse = uCoarse > .5 || uv.x > uReveal;
    if (coarse) uv = (floor(uv * vec2(42., 24.)) + .5) / vec2(42., 24.);
    float x = uv.x + sin(uTime * .08) * .02;
    vec3 color = mix(vec3(.67, .24, .10), vec3(.035, .026, .07), smoothstep(.15, 1., uv.y));
    vec2 sun = (uv - vec2(.72, .72)) * vec2(1.65, 1.);
    float distanceToSun = length(sun);
    color += vec3(.30, .105, .025) * exp(-distanceToSun * 7.);
    color = mix(color, vec3(1., .72, .39), 1. - smoothstep(.091, .098, distanceToSun));
    for (int layer = 0; layer < 5; layer++) {
      float l = 4. - float(layer);
      float ridge = .24 + l * .075 + noise(x * (5. + l * 2.) + l * 42.) * (.13 + l * .016);
      ridge += noise(x * 29. + l * 3.) * .045;
      float mask = 1. - smoothstep(ridge - .004, ridge + .004, uv.y);
      vec3 mountain = mix(vec3(.028, .021, .045), vec3(.23, .09, .12), l / 4.);
      mountain += vec3(.12, .06, .035) * noise(x * 70. + uv.y * 60.) * smoothstep(ridge - .12, ridge, uv.y);
      float snow = smoothstep(ridge - .04, ridge, uv.y) * step(2.5, l);
      mountain = mix(mountain, vec3(.65, .42, .33), snow * .75);
      color = mix(color, mountain, mask);
    }
    float water = 1. - smoothstep(.18, .185, uv.y);
    float reflection = exp(-abs(uv.x - .72) * 10.) * (.35 + .3 * sin(uv.y * 160. - uTime * .4));
    vec3 lake = vec3(.08, .075, .115) + vec3(.45, .23, .12) * reflection;
    lake += .024 * sin(uv.y * 280. + noise(x * 40.) * 3. + uTime * .6);
    color = mix(color, lake, water);
    if (coarse) {
      color = floor(color * 16.) / 16.;
      color *= .84;
      vec2 cell = fract(vUv * vec2(42., 24.));
      color *= .88 + .12 * step(.055, min(cell.x, cell.y));
    }
    float vignette = 1. - .25 * length((vUv - .5) * 1.4);
    if (uReveal < .99 && uCoarse < .5) {
      color += vec3(.9, .36, .09) * exp(-abs(vUv.x - uReveal) * 140.);
    }
    gl_FragColor = vec4(color * vignette, 1.);
  }
`

function makeChipTexture() {
  const canvas = document.createElement("canvas")
  canvas.width = 768
  canvas.height = 768
  const context = canvas.getContext("2d")
  if (!context) throw new Error("Canvas unavailable")
  context.fillStyle = "#18130f"
  context.fillRect(0, 0, 768, 768)
  context.strokeStyle = "#59402a"
  context.lineWidth = 2
  for (let i = 0; i < 12; i++) {
    const offset = 48 + i * 60
    context.beginPath()
    context.moveTo(offset, 0)
    context.lineTo(offset, 130 + (i % 3) * 35)
    context.lineTo(384, 260)
    context.moveTo(offset, 768)
    context.lineTo(offset, 638 - (i % 3) * 35)
    context.lineTo(384, 508)
    context.stroke()
    context.fillStyle = "#c78b4f"
    context.fillRect(offset - 3, 66, 6, 6)
    context.fillRect(offset - 3, 696, 6, 6)
  }
  context.fillStyle = "#201a14"
  context.fillRect(98, 244, 572, 280)
  context.strokeStyle = "#a66b36"
  context.strokeRect(98, 244, 572, 280)
  context.fillStyle = "#ff974d"
  context.font = "700 33px monospace"
  context.textAlign = "center"
  context.fillText("ON-DEVICE", 384, 306)
  context.font = "800 83px sans-serif"
  context.fillStyle = "#f8eee2"
  context.fillText("REFEREE", 384, 407)
  context.font = "500 27px monospace"
  context.fillStyle = "#b28b63"
  context.fillText("GPU ENHANCEMENT", 384, 474)
  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  return texture
}

function makeGlowTexture() {
  const canvas = document.createElement("canvas")
  canvas.width = canvas.height = 128
  const context = canvas.getContext("2d")
  if (!context) throw new Error("Canvas unavailable")
  const gradient = context.createRadialGradient(64, 64, 0, 64, 64, 64)
  gradient.addColorStop(0, "rgba(255, 137, 56, .6)")
  gradient.addColorStop(.25, "rgba(255, 109, 28, .16)")
  gradient.addColorStop(1, "rgba(255, 109, 28, 0)")
  context.fillStyle = gradient
  context.fillRect(0, 0, 128, 128)
  return new THREE.CanvasTexture(canvas)
}

export function createShowcaseScene(host: HTMLDivElement, options: SceneOptions): ShowcaseScene {
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" })
  const mobile = host.clientWidth < 640
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, mobile ? 1.25 : 1.75))
  renderer.setClearColor(0x000000, 0)
  renderer.toneMapping = THREE.ACESFilmicToneMapping
  renderer.toneMappingExposure = 1.25
  const scene = new THREE.Scene()
  const camera = new THREE.OrthographicCamera(-6.6, 6.6, 2.8, -2.8, .1, 60)
  camera.position.set(0, 2.6, 14)
  camera.lookAt(0, 0, 0)
  const world = new THREE.Group()
  scene.add(world)

  scene.add(new THREE.AmbientLight(0xffdebb, 1.6))
  const light = new THREE.DirectionalLight(0xffd1aa, 4)
  light.position.set(2, 5, 7)
  scene.add(light)
  const rim = new THREE.DirectionalLight(0x9896ca, 3)
  rim.position.set(-4, 2, -2)
  scene.add(rim)
  const coreLight = new THREE.PointLight(0xff7026, 8, 8)
  coreLight.position.set(0, .6, 1.5)
  scene.add(coreLight)

  const darkMetal = new THREE.MeshStandardMaterial({ color: 0x242326, metalness: .85, roughness: .28 })
  const orange = new THREE.MeshBasicMaterial({ color: new THREE.Color(2.3, .66, .18), toneMapped: false })
  const mutedOrange = new THREE.LineBasicMaterial({ color: 0xd9814a, transparent: true, opacity: .42 })
  const landscapeMaterials: THREE.ShaderMaterial[] = []

  function screen(x: number, width: number, height: number, coarse: boolean) {
    const group = new THREE.Group()
    group.position.set(x, coarse ? -.02 : .16, 0)
    group.rotation.y = coarse ? .17 : -.17
    group.rotation.z = coarse ? -.045 : .035
    const chassis = new THREE.Mesh(new RoundedBoxGeometry(width, height, .13, 3, .065), darkMetal)
    group.add(chassis)
    const edge = new THREE.LineSegments(new THREE.EdgesGeometry(chassis.geometry, 30), new THREE.LineBasicMaterial({ color: coarse ? 0x8c6a54 : 0xf6aa73, transparent: true, opacity: .45 }))
    group.add(edge)
    const landscape = new THREE.ShaderMaterial({
      vertexShader,
      fragmentShader: landscapeShader,
      uniforms: { uTime: { value: 0 }, uCoarse: { value: coarse ? 1 : 0 }, uReveal: { value: 1 } },
    })
    landscapeMaterials.push(landscape)
    const panel = new THREE.Mesh(new THREE.PlaneGeometry(width - .16, height - .34), landscape)
    panel.position.set(0, -.07, .071)
    group.add(panel)
    for (let i = 0; i < 3; i++) {
      const dot = new THREE.Mesh(new THREE.CircleGeometry(.019, 12), i === 0 ? orange : new THREE.MeshBasicMaterial({ color: 0x62544d }))
      dot.position.set(-width / 2 + .14 + i * .075, height / 2 - .1, .08)
      group.add(dot)
    }
    const progress = new THREE.Mesh(new THREE.PlaneGeometry(width - .3, .012), new THREE.MeshBasicMaterial({ color: 0xb78358, transparent: true, opacity: .55 }))
    progress.position.set(0, -height / 2 + .09, .082)
    group.add(progress)
    world.add(group)
    return group
  }

  const source = screen(-4.2, 2.7, 1.82, true)
  const output = screen(4.1, 3.15, 2.13, false)
  const core = new THREE.Group()
  core.position.set(0, .1, .35)
  core.rotation.set(.1, -.2, -.04)
  world.add(core)
  const chipBody = new THREE.Mesh(new RoundedBoxGeometry(1.56, 1.56, .25, 3, .08), darkMetal)
  core.add(chipBody)
  const chipTexture = makeChipTexture()
  const chipFace = new THREE.Mesh(new THREE.PlaneGeometry(1.43, 1.43), new THREE.MeshBasicMaterial({ map: chipTexture }))
  chipFace.position.z = .131
  core.add(chipFace)
  const chipEdge = new THREE.LineSegments(new THREE.EdgesGeometry(new THREE.BoxGeometry(1.6, 1.6, .26)), orange)
  core.add(chipEdge)
  const pinGeometry = new THREE.BoxGeometry(.075, .15, .07)
  const pinMaterial = new THREE.MeshStandardMaterial({ color: 0xc59660, metalness: .9, roughness: .32 })
  const pins = new THREE.InstancedMesh(pinGeometry, pinMaterial, 40)
  const pinTransform = new THREE.Object3D()
  for (let side = 0; side < 4; side++) {
    for (let pin = 0; pin < 10; pin++) {
      const offset = (pin - 4.5) * .13
      pinTransform.position.set(side % 2 === 0 ? offset : (side === 1 ? .86 : -.86), side % 2 === 0 ? (side === 0 ? .86 : -.86) : offset, 0)
      pinTransform.rotation.z = side % 2 === 0 ? 0 : Math.PI / 2
      pinTransform.updateMatrix()
      pins.setMatrixAt(side * 10 + pin, pinTransform.matrix)
    }
  }
  core.add(pins)

  const orbit = new THREE.Group()
  orbit.position.copy(core.position)
  world.add(orbit)
  const rings: THREE.Group[] = []
  for (let i = 0; i < 3; i++) {
    const ring = new THREE.Group()
    ring.rotation.set(.35 + i * .6, .5 + i * .8, i * .7)
    const radius = 1.28 + i * .12
    const rail = new THREE.Mesh(new THREE.TorusGeometry(radius, .007, 5, 100), new THREE.MeshBasicMaterial({ color: 0xb77545, transparent: true, opacity: .23 }))
    ring.add(rail)
    const arc = new THREE.Mesh(new THREE.TorusGeometry(radius, .015, 6, 50, Math.PI * .6), orange)
    ring.add(arc)
    const bead = new THREE.Mesh(new THREE.SphereGeometry(.035, 8, 8), orange)
    bead.position.x = radius
    ring.add(bead)
    orbit.add(ring)
    rings.push(ring)
  }

  const glowTexture = makeGlowTexture()
  const glow = new THREE.Sprite(new THREE.SpriteMaterial({ map: glowTexture, transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, opacity: .3 }))
  glow.position.set(0, .1, -.6)
  glow.scale.set(6, 6, 1)
  world.add(glow)

  const floor = new THREE.Mesh(new THREE.PlaneGeometry(24, 18), new THREE.ShaderMaterial({
    vertexShader,
    fragmentShader: /* glsl */`
      varying vec2 vUv;
      void main() {
        vec2 p = vUv * vec2(32., 24.);
        vec2 grid = abs(fract(p - .5) - .5) / fwidth(p);
        float line = 1. - min(min(grid.x, grid.y), 1.);
        float fade = max(0., 1. - length((vUv - .5) * 2.3));
        gl_FragColor = vec4(.48, .30, .19, line * fade * .065);
      }
    `,
    transparent: true,
    depthWrite: false,
    side: THREE.DoubleSide,
  }))
  floor.rotation.x = -Math.PI / 2
  floor.position.y = -1.43
  world.add(floor)

  const inputCurve = new THREE.CubicBezierCurve3(new THREE.Vector3(-2.82, -.05, .04), new THREE.Vector3(-1.9, -.05, .2), new THREE.Vector3(-1.9, .12, .4), new THREE.Vector3(-.8, .1, .4))
  const outputCurve = new THREE.CubicBezierCurve3(new THREE.Vector3(.8, .1, .4), new THREE.Vector3(1.7, .15, .5), new THREE.Vector3(1.65, .25, 0), new THREE.Vector3(2.53, .2, .04))
  for (const curve of [inputCurve, outputCurve]) {
    for (let strand = -3; strand <= 3; strand++) {
      const points = curve.getPoints(60).map((point, i) => {
        point.y += strand * .075 * Math.sin(i / 60 * Math.PI)
        point.z += strand * .035
        return point
      })
      world.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), mutedOrange))
    }
  }

  const packets = new THREE.InstancedMesh(new THREE.BoxGeometry(.062, .062, .062), orange, 28)
  packets.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
  packets.frustumCulled = false
  world.add(packets)
  const detailCount = mobile ? 100 : 210
  const detailPositions = new Float32Array(detailCount * 3)
  const detailGeometry = new THREE.BufferGeometry()
  detailGeometry.setAttribute("position", new THREE.BufferAttribute(detailPositions, 3).setUsage(THREE.DynamicDrawUsage))
  const details = new THREE.Points(detailGeometry, new THREE.PointsMaterial({ color: new THREE.Color(2.5, 1.1, .4), size: .027, transparent: true, opacity: .9, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false }))
  details.frustumCulled = false
  world.add(details)

  const stars = new Float32Array(90 * 3)
  for (let i = 0; i < 90; i++) {
    // Deterministic placement avoids a different constellation on each mount.
    stars[i * 3] = Math.sin(i * 127.1) * 8
    stars[i * 3 + 1] = Math.cos(i * 311.7) * 3.2
    stars[i * 3 + 2] = -2 - (i % 5)
  }
  const starField = new THREE.Points(new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(stars, 3)), new THREE.PointsMaterial({ color: 0xeec8a6, size: .016, transparent: true, opacity: .35, depthWrite: false }))
  scene.add(starField)

  const composer = new EffectComposer(renderer)
  composer.addPass(new RenderPass(scene, camera))
  const bloom = new UnrealBloomPass(new THREE.Vector2(1, 1), .55, .55, 1.05)
  composer.addPass(bloom)
  const outputPass = new OutputPass()
  composer.addPass(outputPass)

  let paused = options.paused
  let visible = false
  let disposed = false
  let unavailable = false
  let frame = 0
  let time = 0
  let lastTime = 0
  let currentStep = 0
  let focus = 0
  let coreScale = 1
  let sourceScale = 1
  let outputScale = 1
  let reveal = 1
  let pointerX = 0
  let pointerY = 0
  const dummy = new THREE.Object3D()
  const point = new THREE.Vector3()

  function render(delta: number) {
    if (disposed || unavailable) return
    if (!paused) time += delta
    const step = Math.floor(time / 4.5) % 3
    if (step !== currentStep) {
      currentStep = step
      options.onStep(step)
    }
    const target = step === 0 ? -.7 : step === 2 ? .65 : 0
    focus = THREE.MathUtils.damp(focus, target, 2, delta)
    coreScale = THREE.MathUtils.damp(coreScale, step === 1 ? 1.06 : 1, 3, delta)
    sourceScale = THREE.MathUtils.damp(sourceScale, step === 0 ? 1.025 : 1, 3, delta)
    outputScale = THREE.MathUtils.damp(outputScale, step === 2 ? 1.035 : 1, 3, delta)
    core.scale.setScalar(coreScale)
    source.scale.setScalar(sourceScale)
    output.scale.setScalar(outputScale)
    camera.position.x = focus + pointerX * .18
    camera.position.y = 2.6 + pointerY * .12
    camera.lookAt(focus * .45, 0, 0)
    const drift = Math.sin(time * .7) * .065
    source.position.y = -.02 + drift
    output.position.y = .16 + Math.sin(time * .7 + 1) * .065
    core.position.y = .1 + Math.sin(time * .9) * .085
    core.rotation.y = -.2 + Math.sin(time * .3) * .13
    orbit.position.y = core.position.y
    rings.forEach((ring, i) => { ring.rotation.z = time * (.13 + i * .055) + i * .7 })
    glow.material.opacity = .28 + Math.sin(time * 1.6) * .045
    coreLight.intensity = 7 + Math.sin(time * 1.6)
    if (!paused) reveal = step !== 2 ? 1 : Math.min(1, (time % 4.5 + .2) / 1.6)
    landscapeMaterials.forEach((material) => {
      material.uniforms.uTime.value = time
      material.uniforms.uReveal.value = reveal
    })
    for (let i = 0; i < 28; i++) {
      const progress = (i / 28 + time * .20) % 1
      inputCurve.getPoint(progress, point)
      const envelope = Math.sin(progress * Math.PI)
      point.y += Math.sin(i * 2.4 + time * 2) * .13 * envelope
      point.z += Math.cos(i * 1.7) * .11 * envelope
      dummy.position.copy(point)
      dummy.rotation.set(time * .8 + i, time * .4, i)
      dummy.scale.setScalar(.7 + .4 * envelope)
      dummy.updateMatrix()
      packets.setMatrixAt(i, dummy.matrix)
    }
    packets.instanceMatrix.needsUpdate = true
    for (let i = 0; i < detailCount; i++) {
      const progress = (i / detailCount + time * .22) % 1
      outputCurve.getPoint(progress, point)
      const angle = i * 2.399 + time * 1.1
      const spread = Math.sin(progress * Math.PI) * (.1 + (i % 9) * .024)
      detailPositions[i * 3] = point.x
      detailPositions[i * 3 + 1] = point.y + Math.sin(angle) * spread
      detailPositions[i * 3 + 2] = point.z + Math.cos(angle) * spread
    }
    detailGeometry.attributes.position.needsUpdate = true
    composer.render()
  }

  function tick(now: number) {
    frame = 0
    if (disposed || unavailable || paused || !visible || document.hidden) return
    const delta = lastTime ? Math.min((now - lastTime) / 1000, .05) : 0
    lastTime = now
    render(delta)
    frame = window.requestAnimationFrame(tick)
  }

  function syncLoop() {
    window.cancelAnimationFrame(frame)
    frame = 0
    lastTime = 0
    if (!disposed && !unavailable && !paused && visible && !document.hidden) frame = window.requestAnimationFrame(tick)
  }

  function resize() {
    const width = host.clientWidth
    const height = host.clientHeight
    if (!width || !height || disposed || unavailable) return
    // Keep the full pipeline in view, including on narrow phone screens.
    const halfWidth = width < 640 ? 6.65 : 6.5
    const halfHeight = halfWidth * height / width
    // The canvas extends behind the introductory copy. Shift its frustum so
    // the pipeline keeps its position while the glow can continue above it.
    const stageHeight = host.parentElement?.clientHeight ?? height
    const verticalOffset = (height - stageHeight) * halfWidth / width
    camera.left = -halfWidth
    camera.right = halfWidth
    camera.top = halfHeight + verticalOffset
    camera.bottom = -halfHeight + verticalOffset
    camera.updateProjectionMatrix()
    renderer.setSize(width, height)
    composer.setSize(width, height)
    render(0)
  }

  function pointerMove(event: PointerEvent) {
    if (event.pointerType !== "mouse" || paused) return
    const rect = host.getBoundingClientRect()
    pointerX = (event.clientX - rect.left) / rect.width - .5
    pointerY = (event.clientY - rect.top) / rect.height - .5
  }
  function pointerLeave() { pointerX = pointerY = 0 }
  function contextLost(event: Event) {
    event.preventDefault()
    unavailable = true
    syncLoop()
    options.onUnavailable()
  }

  const resizeObserver = new ResizeObserver(resize)
  const visibilityObserver = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting
    syncLoop()
  })
  host.appendChild(renderer.domElement)
  resizeObserver.observe(host)
  visibilityObserver.observe(host)
  document.addEventListener("visibilitychange", syncLoop)
  host.addEventListener("pointermove", pointerMove)
  host.addEventListener("pointerleave", pointerLeave)
  renderer.domElement.addEventListener("webglcontextlost", contextLost)
  resize()

  return {
    setPaused(value) {
      paused = value
      render(0)
      syncLoop()
    },
    selectStep(step) {
      time = Math.max(0, Math.min(2, step)) * 4.5
      if (paused) {
        focus = step === 0 ? -.7 : step === 2 ? .65 : 0
        reveal = 1
      }
      render(0)
    },
    dispose() {
      disposed = true
      syncLoop()
      resizeObserver.disconnect()
      visibilityObserver.disconnect()
      document.removeEventListener("visibilitychange", syncLoop)
      host.removeEventListener("pointermove", pointerMove)
      host.removeEventListener("pointerleave", pointerLeave)
      renderer.domElement.removeEventListener("webglcontextlost", contextLost)
      const geometries = new Set<THREE.BufferGeometry>()
      const materials = new Set<THREE.Material>()
      scene.traverse((object) => {
        if (object instanceof THREE.Mesh || object instanceof THREE.Line || object instanceof THREE.Points) {
          geometries.add(object.geometry)
          const owned = Array.isArray(object.material) ? object.material : [object.material]
          owned.forEach((material) => materials.add(material))
          if (object instanceof THREE.InstancedMesh) object.dispose()
        } else if (object instanceof THREE.Sprite) {
          materials.add(object.material)
        }
      })
      geometries.forEach((geometry) => geometry.dispose())
      materials.forEach((material) => material.dispose())
      chipTexture.dispose()
      glowTexture.dispose()
      bloom.dispose()
      outputPass.dispose()
      composer.dispose()
      renderer.dispose()
      renderer.forceContextLoss()
      renderer.domElement.remove()
    },
  }
}
