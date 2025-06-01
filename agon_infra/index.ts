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

const appLabels = { app: "nginx" };
const deployment = new k8s.apps.v1.Deployment("nginx-deployment", {
	metadata: { name: "nginx" },
	spec: {
		selector: { matchLabels: appLabels },
		replicas: 1,
		template: {
			metadata: { labels: appLabels },
			spec: {
				containers: [
					{
						name: "nginx",
						image: "nginx:alpine",
						ports: [{ containerPort: 80 }],
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const service = new k8s.core.v1.Service("nginx-service", {
	metadata: { name: "nginx" },
	spec: {
		selector: appLabels,
		ports: [{ port: 80, targetPort: 80 }],
	},
}, { provider: k8sProvider });

const fullDomain = `nginx.${baseDomain}`;

const certificate = new k8s.apiextensions.CustomResource("nginx-app-cert", {
    apiVersion: "cert-manager.io/v1",
    kind: "Certificate",
    metadata: { namespace: 'default', name: "nginx-app-cert" },
    spec: {
        secretName: "nginx-app-cert",
        issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
        commonName: fullDomain,
        dnsNames: [fullDomain],
    },
}, { provider: k8sProvider });

const ingress = new k8s.networking.v1.Ingress("nginx-ingress", {
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
            secretName: "nginx-app-cert",
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
                            port: { number: 80 },
                        },
                    },
                }],
            },
        }],
    },
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const ingressDomain = fullDomain;
