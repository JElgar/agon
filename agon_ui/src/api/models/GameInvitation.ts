/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { InvitationStatus } from './InvitationStatus';
export type GameInvitation = {
    game_id: string;
    user_id: string;
    status: InvitationStatus;
    invited_at: string;
    responded_at?: string;
};

