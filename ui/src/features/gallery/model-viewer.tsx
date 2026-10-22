import { useEffect, useRef } from 'react'
import {
  Box3,
  DirectionalLight,
  DoubleSide,
  HemisphereLight,
  LoadingManager,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  Points,
  PointsMaterial,
  Scene,
  SkinnedMesh,
  Sphere,
  Texture,
  Vector3,
  WebGLRenderer,
  type BufferGeometry,
  type Material,
  type Object3D,
} from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { FBXLoader } from 'three/examples/jsm/loaders/FBXLoader.js'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { PLYLoader } from 'three/examples/jsm/loaders/PLYLoader.js'
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js'

import { gateway, type GalleryEntryDto } from '@/shared/ipc/gateway'

import { extensionOf } from './format'

/** A `data:` URL whose payload cannot be decoded, so any request for it fails. */
export const BLOCKED_URL = 'data:;base64,%'

const SURFACE = 0xb4b8c0

/**
 * The only URLs a model may pull in: its own embedded data (`blob:`, `data:`) and files of the
 * same gallery area. Anything else becomes an undecodable `data:` URL, so a glTF that names a
 * remote or local resource fails to load instead of fetching it.
 */
export function allowedUrl(url: string, areaPrefix: string): string {
  if (url.startsWith('blob:') || url.startsWith('data:')) return url
  try {
    // Parsing resolves `..` segments first, so a path cannot climb out of the area folder.
    const resolved = new URL(url).href
    if (resolved.startsWith(areaPrefix)) return resolved
  } catch {
    // Not an absolute URL, so it is not one of ours.
  }
  return BLOCKED_URL
}

function plyObject(geometry: BufferGeometry): Object3D {
  const colored = geometry.hasAttribute('color')
  const color = colored ? 0xffffff : SURFACE
  if (geometry.index === null) {
    // Faces are optional in PLY; without them the file is a point cloud.
    geometry.computeBoundingSphere()
    const radius = geometry.boundingSphere?.radius ?? 1
    return new Points(
      geometry,
      new PointsMaterial({ color, vertexColors: colored, size: Math.max(radius, 1e-6) / 200 }),
    )
  }
  if (!geometry.hasAttribute('normal')) geometry.computeVertexNormals()
  return new Mesh(
    geometry,
    new MeshStandardMaterial({ color, vertexColors: colored, roughness: 0.6, metalness: 0.1, side: DoubleSide }),
  )
}

async function loadModel(extension: string, url: string, manager: LoadingManager): Promise<Object3D> {
  switch (extension) {
    case '.glb':
    case '.gltf':
      return (await new GLTFLoader(manager).loadAsync(url)).scene
    case '.obj':
      return new OBJLoader(manager).loadAsync(url)
    case '.fbx':
      return new FBXLoader(manager).loadAsync(url)
    case '.stl': {
      const geometry = await new STLLoader(manager).loadAsync(url)
      geometry.computeVertexNormals()
      return new Mesh(
        geometry,
        new MeshStandardMaterial({ color: SURFACE, roughness: 0.6, metalness: 0.1, side: DoubleSide }),
      )
    }
    case '.ply':
      return plyObject(await new PLYLoader(manager).loadAsync(url))
    default:
      throw new Error('Unsupported 3D format.')
  }
}

function isTexture(value: unknown): value is Texture {
  return typeof value === 'object' && value !== null && 'isTexture' in value
}

function disposeTexture(texture: Texture): void {
  if (typeof ImageBitmap !== 'undefined' && texture.image instanceof ImageBitmap) {
    texture.image.close()
  }
  texture.dispose()
}

function disposeMaterial(material: Material): void {
  const values: unknown[] = Object.values(material)
  values.filter(isTexture).forEach(disposeTexture)
  material.dispose()
}

/** Frees the GPU buffers and decoded images of everything under `root`. */
function disposeObject(root: Object3D): void {
  root.traverse((node) => {
    if (node instanceof SkinnedMesh) node.skeleton.dispose()
    // Meshes, points and lines all carry a geometry and one or more materials.
    const { geometry, material } = node as Partial<Mesh>
    geometry?.dispose()
    if (material) (Array.isArray(material) ? material : [material]).forEach(disposeMaterial)
  })
}

interface Props {
  entry: GalleryEntryDto
  /** Called, once per failure, when the file cannot be shown. Keep it stable. */
  onError: () => void
}

/** An orbitable 3D preview. Lazy-loaded so three.js stays out of the main bundle. */
export default function ModelViewer({ entry, onError }: Props) {
  const host = useRef<HTMLDivElement>(null)
  const { area, path } = entry.location
  const { name, revision } = entry

  useEffect(() => {
    const container = host.current
    if (!container) return
    let disposed = false
    let frame = 0

    let renderer: WebGLRenderer
    try {
      renderer = new WebGLRenderer({ antialias: true, alpha: true })
    } catch {
      // No WebGL in this webview.
      onError()
      return
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    renderer.domElement.className = 'block h-full w-full'
    container.appendChild(renderer.domElement)

    const scene = new Scene()
    const camera = new PerspectiveCamera(45, 1, 0.1, 1000)
    scene.add(new HemisphereLight(0xffffff, 0x444444, 1.5))
    // Attached to the camera, so the model stays lit from the viewer's side while orbiting.
    const light = new DirectionalLight(0xffffff, 2)
    light.position.set(1, 2, 3)
    camera.add(light)
    scene.add(camera)

    const controls = new OrbitControls(camera, renderer.domElement)
    controls.enableDamping = true

    const resize = () => {
      const width = Math.max(1, container.clientWidth)
      const height = Math.max(1, container.clientHeight)
      renderer.setSize(width, height, false)
      camera.aspect = width / height
      camera.updateProjectionMatrix()
    }
    resize()
    const observer = new ResizeObserver(resize)
    observer.observe(container)

    const tick = () => {
      frame = requestAnimationFrame(tick)
      controls.update()
      renderer.render(scene, camera)
    }
    tick()

    const fit = (box: Box3) => {
      const sphere = box.getBoundingSphere(new Sphere())
      const halfVertical = (camera.fov * Math.PI) / 360
      const halfHorizontal = Math.atan(Math.tan(halfVertical) * camera.aspect)
      const distance = Math.max(sphere.radius, 1e-6) / Math.sin(Math.min(halfVertical, halfHorizontal))
      camera.near = distance / 100
      camera.far = distance * 100
      camera.updateProjectionMatrix()
      camera.position.copy(sphere.center).addScaledVector(new Vector3(1, 0.6, 1).normalize(), distance * 1.1)
      controls.target.copy(sphere.center)
      controls.update()
    }

    const manager = new LoadingManager()
    const areaPrefix = `${gateway.galleryPreviewBase()}${area}/`
    manager.setURLModifier((url) => allowedUrl(url, areaPrefix))
    const url = gateway.galleryPreviewUrl({ area, path }, revision)
    loadModel(extensionOf(name).toLowerCase(), url, manager)
      .then((object) => {
        if (disposed) {
          disposeObject(object)
          return
        }
        const box = new Box3().setFromObject(object)
        if (box.isEmpty()) {
          disposeObject(object)
          onError()
          return
        }
        scene.add(object)
        fit(box)
      })
      .catch(() => {
        if (!disposed) onError()
      })

    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      observer.disconnect()
      controls.dispose()
      disposeObject(scene)
      renderer.dispose()
      renderer.domElement.remove()
    }
  }, [area, path, name, revision, onError])

  return <div ref={host} className="h-full w-full" />
}
