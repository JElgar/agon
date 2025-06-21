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

// https://github.com/temporalio/helm-charts#install-with-your-own-postgresql
// helm install \
//   --repo https://go.temporal.io/helm-charts \
//   -f values/values.postgresql.yaml \
//   --set elasticsearch.enabled=false \
//   --set server.config.persistence.default.sql.user=postgresql_user \
//   --set server.config.persistence.default.sql.password=postgresql_password \
//   --set server.config.persistence.visibility.sql.user=postgresql_user \
//   --set server.config.persistence.visibility.sql.password=postgresql_password \
//   --set server.config.persistence.default.sql.host=postgresql_host \
//   --set server.config.persistence.visibility.sql.host=postgresql_host \
//   temporaltest temporal --timeout 900s

const temporalDb = new k8s.apiextensions.CustomResource("temporal-db", {
	apiVersion: "postgresql.cnpg.io/v1",
	kind: "Cluster",
	metadata: {
		name: "temporal",
	},
	spec: {
		instances: 1,
		storage: {
			size: "1Gi",
		},
		monitoring: {
			enablePodMonitor: true,
		},
	},
}, { provider: k8sProvider, dependsOn: [cloudnativePg] });

const temporalVisibilityDb = new k8s.apiextensions.CustomResource("temporal-visibility-db", {
	apiVersion: "postgresql.cnpg.io/v1",
	kind: "Cluster",
	metadata: {
		name: "temporal-visibility",
	},
	spec: {
		instances: 1,
		storage: {
			size: "1Gi",
		},
		monitoring: {
			enablePodMonitor: true,
		},
	},
}, { provider: k8sProvider, dependsOn: [cloudnativePg] });

const temporalDbSecretName = temporalDb.metadata.name.apply(value => `${value}-app`)
const temporalVisibilityDbSecretName = temporalVisibilityDb.metadata.name.apply(value => `${value}-app`)
// 
// const temporalDatabaseSecretData = k8s.core.v1.Secret.get(
// 	'temporal-db-secret', temporalDbSecretName.apply(value => `default/${value}`),
// 	{ dependsOn: [temporalDb], provider: k8sProvider }
// ).data
// 
// const temporalVisibilityDatabaseSecretData = k8s.core.v1.Secret.get(
// 	'temporal-visibility-db-secret', temporalVisibilityDbSecretName.apply(value => `default/${value}`),
// 	{ dependsOn: [temporalVisibilityDb], provider: k8sProvider }
// ).data

// const temporalDatabaseConnectionDetails = pulumi.all([temporalDbSecretName, temporalDatabaseSecretData]).apply(([name, data]) => {
// 	return {
// 		host: data['hostname'],
// 		user: data['username'],
// 		existingSecret: name,
// 		database: data['dbname'],
// 	};
// })
// 
// const temporalVisibilityConnectionDetails = pulumi.all([temporalVisibilityDbSecretName, temporalVisibilityDatabaseSecretData]).apply(([name, data]) => {
// 	return {
// 		host: data['hostname'],
// 		user: data['username'],
// 		existingSecret: name,
// 		database: data['dbname'],
// 	};
// })

const temporalDbConnectionConfig = {
	maxConns: 20,
	maxIdleConns: 20,
	maxConnLifetime: "1h",
	driver: 'postgres12',
};

const dbConnectionDetails = {
	host: "temporal-rw",
	port: 5432,
	database: "app",
	user: "app",
	existingSecret: "temporal-app",
}

const visibilityDbConnectionDetails = {
	host: "temporal-visibility-rw",
	port: 5432,
	database: "app",
	user: "app",
	existingSecret: "temporal-visibility-app",
}

// const temporalHelmValues = pulumi.all([temporalDatabaseConnectionDetails, temporalVisibilityConnectionDetails]).apply(([dbConnectionDetails, visibilityDbConnectionDetails]) => {
const temporalHelmValues = {
	server: {
		config: {
			persistence: {
				default: {
					driver: "sql",
					sql: {
						...temporalDbConnectionConfig,
						...dbConnectionDetails,
					},
				},
				visibility: {
					driver: "sql",
					sql: {
						...temporalDbConnectionConfig,
						...visibilityDbConnectionDetails,
					},
				},
			},
		}
	},
	schema: {
		createDatabase: {
			enabled: false,
		},
		setup: {
			enabled: true,
		},
		update: {
			enabled: true,
		},
	},
	postgresql: {
		enabled: true,
	},
	elasticsearch: {
		enabled: false,
	},
	mysql: {
		enabled: false,
	},
	prometheus: {
		enabled: false,
	},
	grafana: {
		enabled: false,
	},
	cassandra: {
		enabled: false,
	},
};

const temporal = new k8s.helm.v4.Chart('temporal', {
	chart: 'temporal',
	version: 'v0.63.0',
	repositoryOpts: {
		repo: 'https://go.temporal.io/helm-charts'
	},
	values: temporalHelmValues,
}, { provider: k8sProvider })

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


const serviceAppLabels = { app: "agon" };
new k8s.apps.v1.Deployment("agon-deployment", {
	metadata: { name: "agon" },
	spec: {
		selector: { matchLabels: serviceAppLabels },
		replicas: 1,
		template: {
			metadata: { labels: serviceAppLabels },
			spec: {
				containers: [
					{
						name: "agon-service",
						image: config.get("agonServiceImage"),
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

const uiAppLabels = { app: "agon-ui" };
new k8s.apps.v1.Deployment("agon-ui-deployment", {
	metadata: { name: "agon-ui" },
	spec: {
		selector: { matchLabels: uiAppLabels },
		replicas: 1,
		template: {
			metadata: { labels: uiAppLabels },
			spec: {
				containers: [
					{
						name: "agon-ui",
						image: config.get("agonUiImage"),
						ports: [{ containerPort: 80 }],
					},
				],
			},
		},
	},
}, { provider: k8sProvider });

const service = new k8s.core.v1.Service("agon-service", {
	metadata: { name: "agon" },
	spec: {
		selector: serviceAppLabels,
		ports: [{ port: 7000, targetPort: 7000 }],
	},
}, { provider: k8sProvider });

const uiService = new k8s.core.v1.Service("agon-ui-service", {
	metadata: { name: "agon-ui" },
	spec: {
		selector: uiAppLabels,
		ports: [{ port: 80, targetPort: 80 }],
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
			"nginx.ingress.kubernetes.io/use-regex": "true",
			"nginx.ingress.kubernetes.io/rewrite-target": "$2",
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
				paths: [
					{
						path: "/api(/|$)(.*)",
						pathType: "Prefix",
						backend: {
							service: {
								name: service.metadata.name,
								port: { number: 7000 },
							},
						},
					},
					{
						path: "/",
						pathType: "Prefix",
						backend: {
							service: {
								name: uiService.metadata.name,
								port: { number: 80 },
							},
						},
					},
				],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const temporalDomain = `temporal.${baseDomain}`;
const temporalCertificate = new k8s.apiextensions.CustomResource("temporal-cert", {
	apiVersion: "cert-manager.io/v1",
	kind: "Certificate",
	metadata: { namespace: 'default', name: "temporal-cert" },
	spec: {
		secretName: "temporal-cert",
		issuerRef: { name: issuer.metadata.name, kind: "ClusterIssuer" },
		commonName: temporalDomain,
		dnsNames: [temporalDomain],
	},
}, { provider: k8sProvider });

const temporalIngress = new k8s.networking.v1.Ingress("temporal-ingress", {
	metadata: {
		namespace: "default",
		annotations: {
			"kubernetes.io/ingress.class": "nginx",
			"cert-manager.io/cluster-issuer": issuer.metadata.name,
		},
	},
	spec: {
		tls: [{
			hosts: [temporalDomain],
			secretName: "temporal-cert",
		}],
		rules: [{
			host: temporalDomain,
			http: {
				paths: [{
					path: "/",
					pathType: "Prefix",
					backend: {
						service: {
							name: "temporal-web",
							port: { number: 8080 },
						},
					},
				}],
			},
		}],
	},
}, { provider: k8sProvider, dependsOn: [ctrl] });

export const agonDomain = fullDomain;
