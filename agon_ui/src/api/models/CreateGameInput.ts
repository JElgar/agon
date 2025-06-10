/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { GameType } from './GameType';
import type { Location } from './Location';
export type CreateGameInput = {
    title: string;
    game_type: GameType;
    location: Location;
    scheduled_time: string;
    duration_minutes: number;
    invited_user_ids: Array<string>;
};

