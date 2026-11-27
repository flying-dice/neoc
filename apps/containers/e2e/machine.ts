/**
 * The machine the Containers journeys find in the fake Docker (`@vos/docker/fake-docker`): a Compose stack that is up,
 * one that is down, and a container on its own; beside their images, one no container was made from and a dangling one.
 */
import type { ImageSeed, Network, Seed } from '@vos/docker/fake-docker'

/** Compose's labels of a service of a stack; `needs` is the service it waits for. */
const compose = (stack: string, service: string, needs?: string): Record<string, string> => ({
  'com.docker.compose.project': stack,
  'com.docker.compose.service': service,
  'com.docker.compose.project.working_dir': `/srv/${stack}`,
  ...(needs === undefined ? {} : { 'com.docker.compose.depends_on': `${needs}:service_started:false` })
})

export const CONTAINERS: Seed[] = [
  { name: 'blog-db-1', image: 'postgres:16', running: true, labels: compose('blog', 'db'), log: ['database system is ready'] },
  {
    name: 'blog-web-1',
    image: 'ghost:5',
    running: true,
    labels: compose('blog', 'web', 'db'),
    ports: { '2368/tcp': '8080' },
    log: ['Ghost is running', 'listening on 2368']
  },
  { name: 'media-jellyfin-1', image: 'jellyfin/jellyfin:10.9', running: false, labels: compose('media', 'jellyfin') },
  { name: 'pihole', image: 'pihole/pihole:2024.07.0', running: true, ports: { '80/tcp': '8053' }, log: ['FTL started'] }
]

export const NETWORKS: Network[] = [
  { name: 'blog_default', labels: { 'com.docker.compose.project': 'blog' } },
  { name: 'media_default', labels: { 'com.docker.compose.project': 'media' } }
]

export const IMAGES: ImageSeed[] = [{ tags: ['redis:7'] }, { tags: [], size: 140_000_000 }]
