import * as pulumi from "@pulumi/pulumi";
import * as hcloud from "@pulumi/hcloud";
import * as command from '@pulumi/command';
import * as k8s from "@pulumi/kubernetes";
import * as cloudflare from "@pulumi/cloudflare";
import * as nginx from "@pulumi/kubernetes-ingress-nginx";

const config = new pulumi.Config();
const subdomainPrefix = pulumi.getStack();
const baseDomain = `${subdomainPrefix}.agon.jameselgar.com`;

// pulumi config set --secret privateKeyBase64 "$(cat ~/.ssh/pulumi_agon_key | base64)"
const privateKeyBase64 = config.requireSecret("privateKeyBase64");
const privateKey = privateKeyBase64.apply(key => Buffer.from(key, 'base64').toString('utf8'));

// pulumi config set --secret publicKey "$(cat ~/.ssh/pulumi_agon_key.pub | tr -d '\n\r')"
const publicKey = config.requireSecret("publicKey");

const sshKey = new hcloud.SshKey("main", {
    name: `${pulumi.getStack()}-ssh-key`,
    publicKey: publicKey,
});

const node = new hcloud.Server("node", {
	name: `${pulumi.getStack()}-node`,
	image: "debian-11",
	serverType: "cx22",
	publicNets: [{
		ipv4Enabled: true,
		ipv6Enabled: true,
	}],
	sshKeys: [sshKey.name],
});

const cloudflareZoneId = '9620974aadec8ffe30d7f699033cf48d';

const dnsRecord = new cloudflare.DnsRecord('record', {
	zoneId: cloudflareZoneId,
	name: `*.${baseDomain}`,
	type: 'A',
	content: node.ipv4Address,
	ttl: 300,
	proxied: false,
});

const connection = {
	host: node.ipv4Address,
	user: 'root',
	privateKey,
};

const installK3s = new command.remote.Command('install-k3s', {
	connection,
	create: 'curl -sfL https://get.k3s.io | sh -s - --disable=traefik',
})

const getKubeconfig = new command.remote.Command('get-kubeconfig', {
	connection,
	create: 'cat /etc/rancher/k3s/k3s.yaml',
}, { dependsOn: [installK3s] })

const kubeconfig = pulumi.all([getKubeconfig.stdout, node.ipv4Address]).apply(([kubeconfig, serverIp]) => {
	return kubeconfig.replace('127.0.0.1', serverIp)
});

const k8sProvider = new k8s.Provider('k3s-provider', {
	kubeconfig,
})

const cloudnativePgNamespace = new k8s.core.v1.Namespace("cloudnative-pg-namespace", {
	metadata: {
		name: "cnpg-system",
	}
}, { provider: k8sProvider });

const cloudnativePg = new k8s.helm.v4.Chart('cloudnative-pg', {
	chart: 'cloudnative-pg',
	version: 'v0.24.0',
	repositoryOpts: {
		repo: 'https://cloudnative-pg.github.io/charts'
	},
	namespace: cloudnativePgNamespace.metadata.apply(metadata => metadata.name),
}, { provider: k8sProvider })

const ctrl = new nginx.IngressController("nginx-ingress", {
    controller: {
        publishService: {
            enabled: true,
        },
    },
}, { provider: k8sProvider });

const certManagerNamespace = new k8s.core.v1.Namespace("cert-manager-namespace", {
	metadata: {
		name: "cert-manager",
	}
}, { provider: k8sProvider });

const certManager = new k8s.helm.v4.Chart('cert-manager', {
	chart: 'cert-manager',
	version: 'v1.17.2',
	repositoryOpts: {
		repo: 'https://charts.jetstack.io'
	},
	namespace: certManagerNamespace.metadata.apply(metadata => metadata.name),
	values: {
		crds: {
			enabled: true,
		}
	}
}, { provider: k8sProvider })

const issuer = new k8s.apiextensions.CustomResource('letsencrypt-prod', {
	apiVersion: 'cert-manager.io/v1',
	kind: 'ClusterIssuer',
	metadata: {
		name: 'letsencrypt-production',
		namespace: 'default',
	},
	spec: {
		acme: {
			email: config.get('letsencrypt-email'),
			server: 'https://acme-v02.api.letsencrypt.org/directory',
			privateKeySecretRef: {
				name: 'letsencrypt-production'
			},
			solvers: [
				{
					http01: {
						ingress: {
							class: 'nginx',
						}
					}
				}
			]
		}
	},
}, { provider: k8sProvider, dependsOn: [certManager, ctrl] })

const dbCluster = new k8s.apiextensions.CustomResource("db", {
    apiVersion: "postgresql.cnpg.io/v1",
    kind: "Cluster",
    metadata: {
        name: "agon-db",
    },
    spec: {
        instances: 2,
        storage: {
            size: "1Gi",
        },
        monitoring: {
            enablePodMonitor: true,
        },
    },
}, { provider: k8sProvider, dependsOn: [cloudnativePg] });

// Create a secret for JWT
const jwtSecret = new k8s.core.v1.Secret("jwt-secret", {
    metadata: {
        name: "jwt-secret",
        namespace: "default",
    },
    type: "Opaque",
    data: {
        "jwt-secret": config.requireSecret("jwtSecret").apply(val => Buffer.from(val).toString("base64")),
    },
}, { provider: k8sProvider });


const appLabels = { app: "agon" };
const deployment = new k8s.apps.v1.Deployment("agon-deployment", {
	metadata: { name: "agon" },
	spec: {
		selector: { matchLabels: appLabels },
		replicas: 1,
		template: {
			metadata: { labels: appLabels },
			spec: {
				containers: [
					{
						name: "agon-service",
						image: "ghcr.io/jelgar/agon_service:sha-e470a2c",
						ports: [{ containerPort: 7000 }],
						env: [
							{
								name: "DATABASE_URL",
								valueFrom: {
									secretKeyRef: {
										name: dbCluster.metadata.name.apply(value => `${value}-app`),
										key: 'uri'
									}
								},
							},
							{
								name: "JWT_SECRET",
								valueFrom: {
									secretKeyRef: {
										name: jwtSecret.metadata.name,
										key: 'jwt-secret',
									}
								},
							},
						]
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const service = new k8s.core.v1.Service("agon-service", {
	metadata: { name: "agon" },
	spec: {
		selector: appLabels,
		ports: [{ port: 7000, targetPort: 7000 }],
	},
}, { provider: k8sProvider });

const fullDomain = `agon.${baseDomain}`;

const certificate = new k8s.apiextensions.CustomResource("agon-cert", {
    apiVersion: "cert-manager.io/v1",
    kind: "Certificate",
    metadata: { namespace: 'default', name: "agon-cert" },
    spec: {
        secretName: "agon-cert",
        issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
        commonName: fullDomain,
        dnsNames: [fullDomain],
    },
}, { provider: k8sProvider });

const ingress = new k8s.networking.v1.Ingress("agon-ingress", {
    metadata: {
        namespace: "default",
        annotations: {
            "kubernetes.io/ingress.class": "nginx",
            "cert-manager.io/cluster-issuer": issuer.metadata.name,
        },
    },
    spec: {
        tls: [{
            hosts: [fullDomain],
            secretName: "agon-cert",
        }],
        rules: [{
            host: fullDomain,
            http: {
                paths: [{
                    path: "/",
                    pathType: "Prefix",
                    backend: {
                        service: {
                            name: service.metadata.name,
                            port: { number: 7000 },
                        },
                    },
                }],
            },
        }],
    },
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const ingressDomain = fullDomain;
